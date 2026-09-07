use nexus_common::models::post::PostKind;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::models::bounded_vec;

/// Comma-separated list of post kinds (min=1, max=7 tokens; duplicates are
/// dropped, order preserved). Uses the same case-sensitive kind vocabulary
/// as the single `kind` filter, including custom values.
#[derive(Debug, ToSchema)]
#[schema(value_type = String, example = "collection,link")]
pub struct PostKinds(pub Vec<PostKind>);

/// `deserialize_csv` requires `TryFrom<String>`; delegate to the strict
/// `FromStr` of the shared kind type.
struct KindToken(PostKind);

impl TryFrom<String> for KindToken {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse::<PostKind>().map(KindToken)
    }
}

impl<'de> Deserialize<'de> for PostKinds {
    fn deserialize<D: serde::de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let tokens = bounded_vec::deserialize_csv::<KindToken, D, 1, 7>(d)?;
        let mut kinds: Vec<PostKind> = Vec::with_capacity(tokens.len());
        for KindToken(kind) in tokens {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        Ok(Self(kinds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<PostKinds, serde_json::Error> {
        serde_json::from_str(&format!("\"{input}\""))
    }

    #[test]
    fn single_kind() {
        let kinds = parse("collection").unwrap();
        assert_eq!(kinds.0, vec![PostKind::Collection]);
    }

    #[test]
    fn multiple_kinds_with_whitespace() {
        let kinds = parse("collection, link").unwrap();
        assert_eq!(kinds.0, vec![PostKind::Collection, PostKind::Link]);
    }

    #[test]
    fn preserves_case() {
        let kinds = parse("Collection").unwrap();
        assert_eq!(kinds.0, vec!["Collection".parse::<PostKind>().unwrap()]);
    }

    #[test]
    fn accepts_custom_value() {
        assert_eq!(parse("event").unwrap().0[0].as_str(), "event");
    }

    #[test]
    fn accepts_literal_unknown() {
        assert_eq!(parse("unknown").unwrap().0, vec![PostKind::Unknown]);
    }

    #[test]
    fn deduplicates_preserving_order() {
        let kinds = parse("collection,link,collection").unwrap();
        assert_eq!(kinds.0, vec![PostKind::Collection, PostKind::Link]);
    }

    #[test]
    fn rejects_empty() {
        assert!(parse("").is_err());
    }

    #[test]
    fn rejects_over_max() {
        assert!(parse("short,long,image,video,link,file,collection,short").is_err());
    }

    #[test]
    fn accepts_all_seven_kinds() {
        let kinds = parse("short,long,image,video,link,file,collection").unwrap();
        assert_eq!(kinds.0.len(), 7);
    }
}
