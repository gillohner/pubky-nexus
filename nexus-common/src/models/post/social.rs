//! Optional generic social text inside opaque custom-post JSON content.
use serde::Deserialize;

use super::MAX_CUSTOM_POST_BYTES;

/// UTF-8 bytes, allowing a 64 KiB description plus a bounded Unicode summary.
pub const MAX_SOCIAL_TEXT_BYTES: usize = 68 * 1024;

#[derive(Deserialize)]
struct ContentEnvelope {
    social: SocialText,
}

#[derive(Deserialize)]
struct SocialText {
    version: u32,
    text: String,
}

/// Only this explicit versioned extension supplies custom mention-bearing text.
/// Malformed, future and oversized extensions retain opaque-content behavior.
pub fn custom_social_text(content: &str) -> Option<String> {
    if content.len() > MAX_CUSTOM_POST_BYTES {
        return None;
    }
    let extension = serde_json::from_str::<ContentEnvelope>(content)
        .ok()?
        .social;
    (extension.version == 1 && extension.text.len() <= MAX_SOCIAL_TEXT_BYTES)
        .then_some(extension.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_only_opted_in_text_not_other_metadata() {
        let content = json!({"social":{"version":1,"text":"human description"}, "calendar_uris":["pubky-other-user"], "organizer":"pubky-another-user"}).to_string();
        assert_eq!(
            custom_social_text(&content).as_deref(),
            Some("human description")
        );
        assert!(custom_social_text(r#"{"description":"pubky-user"}"#).is_none());
    }

    #[test]
    fn invalid_extensions_fall_back_to_opaque_content() {
        for content in [
            "not json",
            "[]",
            r#"{"social":null}"#,
            r#"{"social":{"version":2,"text":"pubky-user"}}"#,
            r#"{"social":{"version":1,"text":4}}"#,
        ] {
            assert!(custom_social_text(content).is_none());
        }
    }

    #[test]
    fn text_limit_measures_utf8_bytes_at_the_boundary() {
        let text = "é".repeat(MAX_SOCIAL_TEXT_BYTES / 2);
        assert!(
            custom_social_text(&json!({"social":{"version":1,"text":text}}).to_string()).is_some()
        );
        assert!(custom_social_text(
            &json!({"social":{"version":1,"text":format!("{text}x")}}).to_string()
        )
        .is_none());
    }
}
