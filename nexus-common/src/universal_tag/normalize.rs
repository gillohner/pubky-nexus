use pubky_app_specs::{ParsedUri, Resource};

pub use pubky_resource_id::{normalize_uri, resource_id};

#[derive(Debug, Clone, PartialEq)]
pub enum UriCategory {
    /// pubky:// URI matching a known schema (posts, users)
    InternalKnown,
    /// pubky:// URI NOT matching any known schema (e.g., eventky events)
    InternalUnknown,
    /// Non-pubky:// URI (https://, nostr:, ipfs://, etc.)
    External,
}

/// Classifies a tag's target URI into one of three categories.
/// Scheme check is case-insensitive per RFC 3986.
pub fn classify_uri(uri: &str) -> UriCategory {
    let is_pubky = uri
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("pubky://"));
    if is_pubky {
        match ParsedUri::try_from(uri) {
            Ok(parsed) if matches!(parsed.resource, Resource::Post(_) | Resource::User) => {
                UriCategory::InternalKnown
            }
            _ => UriCategory::InternalUnknown,
        }
    } else {
        UriCategory::External
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- classify_uri tests --

    #[test]
    fn test_classify_external_https() {
        assert_eq!(
            classify_uri("https://example.com/article"),
            UriCategory::External
        );
    }

    #[test]
    fn test_classify_external_nostr() {
        assert_eq!(classify_uri("nostr:note1abc123"), UriCategory::External);
    }

    #[test]
    fn test_classify_internal_unknown_eventky() {
        // eventky URI is pubky:// but not a recognized pubky.app resource
        assert_eq!(
            classify_uri("pubky://8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo/pub/eventky.app/events/E001"),
            UriCategory::InternalUnknown
        );
    }

    #[test]
    fn test_classify_uppercase_pubky_scheme() {
        // RFC 3986: schemes are case-insensitive
        assert_eq!(
            classify_uri("PUBKY://8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo/pub/eventky.app/events/E001"),
            UriCategory::InternalUnknown
        );
    }
}
