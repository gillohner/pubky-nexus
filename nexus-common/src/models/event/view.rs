use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::EventDetails;
use crate::models::attendee::AttendeeDetails;
use crate::models::tag::event::TagEvent;
use crate::models::tag::traits::TagCollection;
use crate::models::tag::TagDetails;
use crate::types::DynError;

/// Represents an Event with relational data including tags and attendees
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct EventView {
    pub details: EventDetails,
    pub tags: Vec<TagDetails>,
    pub attendees: Vec<AttendeeDetails>,
}

impl EventView {
    /// Retrieves event with tags and attendees
    pub async fn get_by_id(
        author_id: &str,
        event_id: &str,
        viewer_id: Option<&str>,
        limit_tags: Option<usize>,
        limit_taggers: Option<usize>,
        limit_attendees: Option<usize>,
    ) -> Result<Option<Self>, DynError> {
        // Fetch details first
        let details = match EventDetails::get_by_id(author_id, event_id).await? {
            None => return Ok(None),
            Some(details) => details,
        };

        // Fetch tags and attendees concurrently
        let (tags, attendees) = tokio::try_join!(
            TagEvent::get_by_id(
                author_id,
                Some(event_id),
                None,
                limit_tags,
                limit_taggers,
                viewer_id,
                None,
            ),
            AttendeeDetails::get_for_event(author_id, event_id, limit_attendees),
        )?;

        Ok(Some(Self {
            details,
            tags: tags.unwrap_or_default(),
            attendees: attendees.unwrap_or_default(),
        }))
    }
}
