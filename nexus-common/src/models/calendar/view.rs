use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::CalendarDetails;
use crate::db::{fetch_all_rows_from_graph, queries};
use crate::models::tag::calendar::TagCalendar;
use crate::models::tag::traits::TagCollection;
use crate::models::tag::TagDetails;
use crate::types::DynError;

/// Represents a Calendar with relational data including tags and events
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct CalendarView {
    pub details: CalendarDetails,
    pub tags: Vec<TagDetails>,
    /// Event URIs belonging to this calendar (via x_pubky_calendar_uris)
    pub events: Vec<String>,
}

impl CalendarView {
    /// Retrieves calendar with tags and events
    pub async fn get_by_id(
        author_id: &str,
        calendar_id: &str,
        viewer_id: Option<&str>,
        limit_tags: Option<usize>,
        limit_taggers: Option<usize>,
    ) -> Result<Option<Self>, DynError> {
        Self::get_by_id_with_events(
            author_id,
            calendar_id,
            viewer_id,
            limit_tags,
            limit_taggers,
            None, // default event limit
        )
        .await
    }

    /// Retrieves calendar with tags and events, with configurable event limit
    pub async fn get_by_id_with_events(
        author_id: &str,
        calendar_id: &str,
        viewer_id: Option<&str>,
        limit_tags: Option<usize>,
        limit_taggers: Option<usize>,
        limit_events: Option<usize>,
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

        // Fetch event URIs belonging to this calendar
        let events = Self::get_event_uris(author_id, calendar_id, limit_events).await?;

        Ok(Some(Self { details, tags, events }))
    }

    /// Fetches event URIs that belong to this calendar
    async fn get_event_uris(
        author_id: &str,
        calendar_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<String>, DynError> {
        let query = queries::get::get_calendar_event_uris(author_id, calendar_id, limit);
        let rows = fetch_all_rows_from_graph(query).await?;

        let mut event_uris = Vec::new();
        for row in rows {
            if let Ok(uri) = row.get::<String>("event_uri") {
                event_uris.push(uri);
            }
        }

        Ok(event_uris)
    }
}
