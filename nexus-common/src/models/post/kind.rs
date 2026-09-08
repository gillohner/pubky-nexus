use std::{fmt, str::FromStr};

use pubky_app_specs::PubkyAppPostKind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A case-sensitive post kind. Custom values survive indexing and API round trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(try_from = "String", into = "String")]
pub enum PostKind {
    #[default]
    Short,
    Long,
    Image,
    Video,
    Link,
    File,
    Collection,
    /// Legacy rows/notifications may carry the literal `unknown`.
    Unknown,
    Custom(String),
}

impl utoipa::PartialSchema for PostKind {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .description(Some(
                "Case-sensitive post kind, 1–128 bytes; custom kinds are preserved.",
            ))
            .into()
    }
}

impl ToSchema for PostKind {}

impl PostKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Short => "short",
            Self::Long => "long",
            Self::Image => "image",
            Self::Video => "video",
            Self::Link => "link",
            Self::File => "file",
            Self::Collection => "collection",
            Self::Unknown => "unknown",
            Self::Custom(value) => value,
        }
    }

    pub fn known(&self) -> Option<PubkyAppPostKind> {
        self.as_str().parse().ok()
    }
}

impl TryFrom<String> for PostKind {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Commas delimit exclude_kinds; whitespace/control characters do not
        // belong in identifiers. Punctuation otherwise remains application-owned.
        if value.is_empty()
            || value.len() > 128
            || value
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == ',')
        {
            return Err(
                "post kind must be 1–128 bytes without whitespace, controls or commas".into(),
            );
        }
        Ok(match value.as_str() {
            "short" => Self::Short,
            "long" => Self::Long,
            "image" => Self::Image,
            "video" => Self::Video,
            "link" => Self::Link,
            "file" => Self::File,
            "collection" => Self::Collection,
            "unknown" => Self::Unknown,
            _ => Self::Custom(value),
        })
    }
}

impl FromStr for PostKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for PostKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<PostKind> for String {
    fn from(value: PostKind) -> Self {
        value.to_string()
    }
}

impl From<PubkyAppPostKind> for PostKind {
    fn from(value: PubkyAppPostKind) -> Self {
        match value {
            PubkyAppPostKind::Short => Self::Short,
            PubkyAppPostKind::Long => Self::Long,
            PubkyAppPostKind::Image => Self::Image,
            PubkyAppPostKind::Video => Self::Video,
            PubkyAppPostKind::Link => Self::Link,
            PubkyAppPostKind::File => Self::File,
            PubkyAppPostKind::Collection => Self::Collection,
            PubkyAppPostKind::Unknown => Self::Unknown,
        }
    }
}

impl PartialEq<PubkyAppPostKind> for PostKind {
    fn eq(&self, other: &PubkyAppPostKind) -> bool {
        self == &Self::from(other.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_custom_and_known_kinds_without_folding_case() {
        for value in [
            "short",
            "collection",
            "event",
            "Event",
            "app:review",
            "unknown",
        ] {
            let json = serde_json::to_string(value).unwrap();
            let kind: PostKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind.as_str(), value);
            assert_eq!(serde_json::to_string(&kind).unwrap(), json);
        }
        assert_ne!(
            "event".parse::<PostKind>().unwrap(),
            "Event".parse::<PostKind>().unwrap()
        );
    }

    #[test]
    fn rejects_invalid_kind_identifiers() {
        for value in ["", " event", "a,b", "a\nb", &"a".repeat(129)] {
            assert!(value.parse::<PostKind>().is_err());
        }
    }
}
