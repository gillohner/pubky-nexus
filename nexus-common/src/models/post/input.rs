use pubky_app_specs::{
    traits::Validatable, ParsedUri, PubkyAppPost, PubkyAppPostEmbed, PubkyAppPostKind, Resource,
};
use serde::{Deserialize, Deserializer};

use super::PostKind;

pub const MAX_CUSTOM_POST_BYTES: usize = 512 * 1024;

/// Nexus's public-post input boundary. Content stays opaque for custom kinds;
/// built-in kinds keep specs validation. This is a v0-path PoC, not a v1 parser.
#[derive(Debug, Clone, Deserialize)]
pub struct PostInput {
    pub content: String,
    pub kind: PostKind,
    pub parent: Option<String>,
    #[serde(default, deserialize_with = "deserialize_embed")]
    pub embed: Option<String>,
    pub attachments: Option<Vec<String>>,
    pub lock: Option<String>,
}

impl PostInput {
    pub fn from_bytes(bytes: &[u8], id: &str) -> Result<Self, String> {
        let mut post: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if post.kind.known().is_none() && bytes.len() > MAX_CUSTOM_POST_BYTES {
            return Err("custom post exceeds 512 KiB".into());
        }
        // Keep v0 cleanup for the common fields, but never trim custom content.
        let sanitized = PubkyAppPost {
            content: post.content.clone(),
            kind: post.kind.known().unwrap_or(PubkyAppPostKind::Short),
            parent: post.parent.take(),
            attachments: post.attachments.take(),
            lock: post.lock.take(),
            embed: None,
        }
        .sanitize();
        if post.kind.known().is_some() {
            post.content = sanitized.content;
        }
        post.parent = sanitized.parent;
        post.attachments = sanitized.attachments;
        post.lock = sanitized.lock;
        post.validate(id)?;
        Ok(post)
    }

    fn validate(&self, id: &str) -> Result<(), String> {
        if let Some(parent) = &self.parent {
            let uri = ParsedUri::try_from(parent.as_str())?;
            if !matches!(uri.resource, Resource::Post(_)) {
                return Err("post parent must reference a Pubky post".into());
            }
        }
        if self.content == "[DELETED]" {
            return Err("post content cannot be the reserved deletion marker".into());
        }
        if self.content.trim().is_empty() && self.embed.is_none() && self.attachments.is_none() {
            return Err("post must have content, an embed, or attachments".into());
        }

        // Reuse the specs' ID, attachment, lock and built-in content rules.
        // A placeholder removes only custom content and universal embed parsing
        // from that validator. Collection's embed prohibition still applies.
        let known = self.kind.known();
        let validation_post = PubkyAppPost {
            content: if known.is_some() {
                self.content.clone()
            } else {
                "custom".into()
            },
            kind: known.unwrap_or(PubkyAppPostKind::Short),
            parent: self.parent.clone(),
            embed: self.embed.as_ref().map(|_| PubkyAppPostEmbed {
                kind: PubkyAppPostKind::Link,
                uri: "https://example.com/".into(),
            }),
            attachments: self.attachments.clone(),
            lock: self.lock.clone(),
        };
        validation_post.validate(Some(id))
    }
}

impl From<PubkyAppPost> for PostInput {
    fn from(post: PubkyAppPost) -> Self {
        Self {
            content: post.content,
            kind: post.kind.into(),
            parent: post.parent,
            embed: post.embed.map(|embed| embed.uri),
            attachments: post.attachments,
            lock: post.lock,
        }
    }
}

fn deserialize_embed<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Embed {
        Uri(String),
        Legacy(PubkyAppPostEmbed),
    }
    Option::<Embed>::deserialize(deserializer)?
        .map(|embed| match embed {
            Embed::Uri(uri) => universal_embed(&uri),
            // Legacy objects retain v0 validation and URL cleanup; the kind is not
            // carried into the new output shape.
            Embed::Legacy(embed) => {
                if !embed.kind.is_known() {
                    return Err("legacy embed kind is unknown".into());
                }
                let sanitized = PubkyAppPost {
                    embed: Some(embed),
                    ..Default::default()
                }
                .sanitize();
                let uri = sanitized.embed.expect("sanitization preserves embed").uri;
                url::Url::parse(&uri)
                    .map(|_| uri)
                    .map_err(|e| e.to_string())
            }
        })
        .transpose()
        .map_err(serde::de::Error::custom)
}

/// RFC #142's universal URI gate, with the current public Pubky path grammar.
/// Normalization for Resource identity is deliberately a separate operation.
fn universal_embed(value: &str) -> Result<String, String> {
    let value = value.trim_matches(frozen_whitespace);
    if value.chars().count() > 1024
        || value
            .chars()
            .any(|c| c.is_ascii_control() || frozen_whitespace(c))
    {
        return Err("embed must be at most 1024 characters without whitespace or controls".into());
    }
    let (scheme, remainder) = value
        .split_once(':')
        .ok_or("embed must have a URI scheme")?;
    if remainder.is_empty()
        || !scheme
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
    {
        return Err("invalid embed URI scheme or empty target".into());
    }
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "pubky" {
        let url = url::Url::parse(value).map_err(|e| e.to_string())?;
        if url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || !url.path().starts_with("/pub/")
        {
            return Err("Pubky embeds must reference public storage".into());
        }
        url.host_str()
            .ok_or("missing Pubky key")?
            .parse::<pubky::PublicKey>()
            .map_err(|e| e.to_string())?;
    } else if scheme.starts_with("pubky") {
        return Err("reserved Pubky scheme".into());
    } else if matches!(scheme.as_str(), "http" | "https")
        && !remainder
            .strip_prefix("//")
            .is_some_and(|host| !host.is_empty() && !host.starts_with('/'))
    {
        return Err("web embed must contain a host".into());
    }
    Ok(format!("{scheme}:{remainder}"))
}

fn frozen_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{0085}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "003286NSMY490";

    fn parse(value: serde_json::Value) -> Result<PostInput, String> {
        PostInput::from_bytes(&serde_json::to_vec(&value).unwrap(), ID)
    }

    #[test]
    fn custom_content_is_not_trimmed_or_interpreted() {
        let content = format!("  {{\"data\":\"{}\"}}  ", "x".repeat(6000));
        let post = parse(serde_json::json!({"kind":"event", "content":content})).unwrap();
        assert_eq!(post.kind.as_str(), "event");
        assert_eq!(post.content, content);
    }

    #[test]
    fn accepts_string_and_legacy_embeds() {
        for embed in [
            serde_json::json!("https://example.com/a#b"),
            serde_json::json!({"uri":"https://example.com/a#b", "kind":"link"}),
        ] {
            let post =
                parse(serde_json::json!({"kind":"short", "content":"", "embed":embed})).unwrap();
            assert_eq!(post.embed.as_deref(), Some("https://example.com/a#b"));
        }
    }

    #[test]
    fn universal_identifiers_preserve_the_opaque_remainder() {
        for (input, expected) in [
            ("GEO:1,2", "geo:1,2"),
            ("nostr:ABC#x", "nostr:ABC#x"),
            ("ipfs://CID", "ipfs://CID"),
            ("https://Example.com", "https://Example.com"),
        ] {
            assert_eq!(universal_embed(input).unwrap(), expected);
        }
        for invalid in ["https:///", "geo:", "1geo:a", "https://a\nb", "no-scheme"] {
            assert!(universal_embed(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn pubky_embeds_must_be_public() {
        let key = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";
        assert!(universal_embed(&format!("pubky://{key}/pub/mapky/places/one")).is_ok());
        assert!(universal_embed(&format!("pubky://{key}/priv/mapky/places/one")).is_err());
        assert!(universal_embed("pubky://invalid/pub/mapky/places/one").is_err());
        assert!(universal_embed("pubky+private:secret").is_err());
    }

    #[test]
    fn builtin_cleanup_is_unchanged() {
        let content = "x".repeat(2000);
        let post =
            parse(serde_json::json!({"kind":"short", "content":format!("  {content}  ")})).unwrap();
        assert_eq!(post.content, content);
        assert!(parse(serde_json::json!({"kind":"short", "content":" [DELETED] "})).is_err());
    }

    #[test]
    fn custom_size_limit_counts_the_whole_utf8_document() {
        let overhead = serde_json::to_vec(&serde_json::json!({"kind":"event", "content":""}))
            .unwrap()
            .len();
        let content = format!("é{}", "x".repeat(MAX_CUSTOM_POST_BYTES - overhead - 2));
        assert!(parse(serde_json::json!({"kind":"event", "content":content})).is_ok());
        assert!(
            parse(serde_json::json!({"kind":"event", "content":format!("{content}x")})).is_err()
        );
    }

    #[test]
    fn retains_builtin_validation_and_parent_rules() {
        for value in [
            serde_json::json!({"kind":"short", "content":"x".repeat(6000)}),
            serde_json::json!({"kind":"collection", "content":"not json"}),
            serde_json::json!({"kind":"event", "content":"x", "parent":"https://example.com"}),
            serde_json::json!({"kind":"event", "content":{}}),
            serde_json::json!({"kind":"event", "content":"[DELETED]"}),
            serde_json::json!({"kind":"event", "content":"x", "attachments":["javascript:x"]}),
            serde_json::json!({"kind":"event", "content":"x".repeat(MAX_CUSTOM_POST_BYTES)}),
        ] {
            assert!(parse(value).is_err());
        }
    }
}
