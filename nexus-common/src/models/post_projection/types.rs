use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Indexed source fields, independent of application-specific content schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct SourcePost {
    pub kind: String,
    pub content: String,
    pub parent: Option<String>,
    pub embed: Option<String>,
    pub attachments: Option<Vec<String>>,
    pub lock: Option<String>,
}

/// A committed source version. `post: null` is a durable deletion record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct SourceChange {
    /// Decimal string: consumers must not round a Neo4j 64-bit revision in JavaScript.
    pub revision: String,
    pub uri: String,
    pub post: Option<SourcePost>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Checkpoint {
    pub epoch: String,
    pub revision: String,
    /// Lowest replay cursor still valid, inclusive. Older cursors require inventory.
    pub minimum_revision: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InventoryPage {
    #[serde(flatten)]
    pub checkpoint: Checkpoint,
    pub items: Vec<SourceChange>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChangesPage {
    #[serde(flatten)]
    pub checkpoint: Checkpoint,
    pub items: Vec<SourceChange>,
    pub cursor: String,
    pub through: String,
    pub caught_up: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("{0}")]
    Graph(#[from] crate::db::GraphError),
    #[error("Projection reset required: {0}")]
    ResetRequired(&'static str),
    #[error("Invalid projection query: {0}")]
    InvalidInput(&'static str),
}

impl Checkpoint {
    /// Validate against the same locked graph read that supplied the response payload.
    pub fn validate(&self, epoch: &str, cursor: i64) -> Result<(), ProjectionError> {
        if self.epoch != epoch {
            return Err(ProjectionError::ResetRequired("index epoch changed"));
        }
        if cursor < parse_revision(&self.minimum_revision)?
            || cursor > parse_revision(&self.revision)?
        {
            return Err(ProjectionError::ResetRequired(
                "cursor expired or index was restored",
            ));
        }
        Ok(())
    }
}

pub fn parse_revision(value: &str) -> Result<i64, ProjectionError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProjectionError::InvalidInput(
            "revision must be a nonnegative decimal integer",
        ));
    }
    value
        .parse()
        .map_err(|_| ProjectionError::InvalidInput("revision exceeds signed 64-bit range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_do_not_round_above_javascript_integer_limit() {
        assert_eq!(
            parse_revision("9007199254740993").unwrap(),
            9_007_199_254_740_993
        );
        for value in ["", "-1", "+1", "1.1", "9223372036854775808"] {
            assert!(parse_revision(value).is_err());
        }
    }

    #[test]
    fn rejects_expired_future_and_wrong_epoch_cursors() {
        let checkpoint = Checkpoint {
            epoch: "current".into(),
            revision: "12".into(),
            minimum_revision: "5".into(),
        };
        assert!(checkpoint.validate("current", 5).is_ok());
        assert!(checkpoint.validate("current", 12).is_ok());
        for (epoch, revision) in [("old", 7), ("current", 4), ("current", 13)] {
            assert!(matches!(
                checkpoint.validate(epoch, revision),
                Err(ProjectionError::ResetRequired(_))
            ));
        }
    }
}
