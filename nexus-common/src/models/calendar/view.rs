use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::CalendarDetails;
use crate::models::tag::calendar::TagCalendar;
use crate::models::tag::traits::TagCollection;
use crate::models::tag::TagDetails;
use crate::types::DynError;

/// Represents a Calendar with relational data including tags
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct CalendarView {
    pub details: CalendarDetails,
    pub tags: Vec<TagDetails>,
}

impl CalendarView {
    /// Retrieves calendar with tags
    pub async fn get_by_id(
        author_id: &str,
        calendar_id: &str,
        viewer_id: Option<&str>,
        limit_tags: Option<usize>,
        limit_taggers: Option<usize>,
    ) -> Result<Option<Self>, DynError> {
        // Fetch details first
        let details = match CalendarDetails::get_by_id(author_id, calendar_id).await? {
            None => return Ok(None),
            Some(details) => details,
        };

        // Fetch tags
        let tags = TagCalendar::get_by_id(
            author_id,
            Some(calendar_id),
            None,
            limit_tags,
            limit_taggers,
            viewer_id,
            None,
        )
        .await?
        .unwrap_or_default();

        Ok(Some(Self { details, tags }))
    }
}
