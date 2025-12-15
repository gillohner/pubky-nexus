//! Serde helper functions for custom deserialization
//!
//! Provides reusable deserializers for common patterns across API endpoints.

use serde::{de, Deserialize, Deserializer};

/// Deserialize a comma-separated string into a vector of strings
///
/// This deserializer handles optional comma-separated values in query parameters.
/// For example, `tags=dev,free,opensource` becomes `Some(vec!["dev", "free", "opensource"])`.
///
/// # Errors
///
/// Returns an error if the string is empty (which would indicate malformed input).
///
/// # Examples
///
/// ```rust
/// use serde::Deserialize;
/// use nexus_webapi::utils::serde_helpers::deserialize_comma_separated;
///
/// #[derive(Deserialize)]
/// struct Query {
///     #[serde(default, deserialize_with = "deserialize_comma_separated")]
///     tags: Option<Vec<String>>,
/// }
/// ```
pub fn deserialize_comma_separated<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    if let Some(s) = s {
        if s.is_empty() {
            return Err(de::Error::custom("Tags cannot be empty"));
        }
        // Split by comma and trim any excess whitespace
        let tags: Vec<String> = s.split(',').map(|tag| tag.trim().to_string()).collect();
        return Ok(Some(tags));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestQuery {
        #[serde(default, deserialize_with = "deserialize_comma_separated")]
        tags: Option<Vec<String>>,
    }

    #[test]
    fn test_deserialize_comma_separated_with_values() {
        let query = serde_urlencoded::from_str::<TestQuery>("tags=dev,free,opensource").unwrap();
        assert_eq!(
            query.tags,
            Some(vec!["dev".to_string(), "free".to_string(), "opensource".to_string()])
        );
    }

    #[test]
    fn test_deserialize_comma_separated_with_whitespace() {
        let query = serde_urlencoded::from_str::<TestQuery>("tags=dev, free , opensource").unwrap();
        assert_eq!(
            query.tags,
            Some(vec!["dev".to_string(), "free".to_string(), "opensource".to_string()])
        );
    }

    #[test]
    fn test_deserialize_comma_separated_single_value() {
        let query = serde_urlencoded::from_str::<TestQuery>("tags=dev").unwrap();
        assert_eq!(query.tags, Some(vec!["dev".to_string()]));
    }

    #[test]
    fn test_deserialize_comma_separated_none() {
        let query = serde_urlencoded::from_str::<TestQuery>("").unwrap();
        assert_eq!(query.tags, None);
    }

    #[test]
    fn test_deserialize_comma_separated_empty_string_error() {
        let result = serde_urlencoded::from_str::<TestQuery>("tags=");
        assert!(result.is_err());
    }
}
