use crate::db::{
    exec_single_row, execute_graph_operation, fetch_row_from_graph, queries, OperationOutcome,
    RedisOps,
};
use crate::types::DynError;
use chrono::Utc;
use pubky_app_specs::{event_uri_builder, PubkyAppEvent, PubkyId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Represents event data with simplified RFC 5545/7986/9073 fields
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct EventDetails {
    pub id: String,
    pub indexed_at: i64,
    pub author: String,
    pub uri: String,
    // Required RFC 5545 fields
    pub uid: String,
    pub dtstamp: i64,
    pub dtstart: String,
    pub summary: String,
    // Optional RFC 5545 fields
    pub dtend: Option<String>,
    pub duration: Option<String>,
    pub dtstart_tzid: Option<String>,
    pub dtend_tzid: Option<String>,
    pub rrule: Option<String>,
    pub rdate: Option<Vec<String>>,
    pub exdate: Option<Vec<String>>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub location: Option<String>,
    pub geo: Option<String>,
    pub url: Option<String>,
    pub sequence: Option<i32>,
    pub last_modified: Option<i64>,
    pub created: Option<i64>,
    pub recurrence_id: Option<String>,
    // RFC 7986 fields
    pub image_uri: Option<String>,
    pub styled_description: Option<String>,   // Serialized JSON
    // Pubky extensions
    pub x_pubky_calendar_uris: Option<Vec<String>>,
    pub x_pubky_rsvp_access: Option<String>,
}

impl RedisOps for EventDetails {}

impl EventDetails {
    /// Retrieves event details by author ID and event ID, first trying Redis, then Neo4j
    pub async fn get_by_id(
        author_id: &str,
        event_id: &str,
    ) -> Result<Option<EventDetails>, DynError> {
        match Self::get_from_index(author_id, event_id).await? {
            Some(details) => Ok(Some(details)),
            None => {
                let graph_response = Self::get_from_graph(author_id, event_id).await?;
                if let Some(event_details) = graph_response {
                    event_details.put_to_index(author_id, event_id).await?;
                    return Ok(Some(event_details));
                }
                Ok(None)
            }
        }
    }

    pub async fn get_from_index(
        author_id: &str,
        event_id: &str,
    ) -> Result<Option<EventDetails>, DynError> {
        if let Some(event_details) =
            Self::try_from_index_json(&[author_id, event_id], None).await?
        {
            return Ok(Some(event_details));
        }
        Ok(None)
    }

    /// Retrieves the event fields from Neo4j
    pub async fn get_from_graph(
        author_id: &str,
        event_id: &str,
    ) -> Result<Option<EventDetails>, DynError> {
        let query = queries::get::get_event_by_id(author_id, event_id);
        let maybe_row = fetch_row_from_graph(query).await?;

        let Some(row) = maybe_row else {
            return Ok(None);
        };

        let event: EventDetails = row.get("details")?;
        Ok(Some(event))
    }

    pub async fn put_to_index(&self, author_id: &str, event_id: &str) -> Result<(), DynError> {
        self.put_index_json(&[author_id, event_id], None, None)
            .await?;
        // TODO: Add to event streams when implementing stream endpoints
        Ok(())
    }

    pub async fn from_homeserver(
        homeserver_event: PubkyAppEvent,
        author_id: &PubkyId,
        event_id: &String,
    ) -> Result<Self, DynError> {
        // Serialize styled_description to JSON string for storage
        let styled_description = homeserver_event
            .styled_description
            .as_ref()
            .and_then(|sd| serde_json::to_string(sd).ok());

        Ok(EventDetails {
            uri: event_uri_builder(author_id.to_string(), event_id.into()),
            id: event_id.clone(),
            indexed_at: Utc::now().timestamp_millis(),
            author: author_id.to_string(),
            // Required fields
            uid: homeserver_event.uid,
            dtstamp: homeserver_event.dtstamp,
            dtstart: homeserver_event.dtstart,
            summary: homeserver_event.summary,
            // Optional fields
            dtend: homeserver_event.dtend,
            duration: homeserver_event.duration,
            dtstart_tzid: homeserver_event.dtstart_tzid,
            dtend_tzid: homeserver_event.dtend_tzid,
            rrule: homeserver_event.rrule,
            rdate: homeserver_event.rdate,
            exdate: homeserver_event.exdate,
            description: homeserver_event.description,
            status: homeserver_event.status,
            location: homeserver_event.location,
            geo: homeserver_event.geo,
            url: homeserver_event.url,
            sequence: homeserver_event.sequence,
            last_modified: homeserver_event.last_modified,
            created: homeserver_event.created,
            recurrence_id: homeserver_event.recurrence_id,
            // RFC 7986
            image_uri: homeserver_event.image_uri,
            styled_description,
            // Pubky extensions
            x_pubky_calendar_uris: homeserver_event.x_pubky_calendar_uris,
            x_pubky_rsvp_access: homeserver_event.x_pubky_rsvp_access,
        })
    }

    pub async fn reindex(author_id: &str, event_id: &str) -> Result<(), DynError> {
        match Self::get_from_graph(author_id, event_id).await? {
            Some(details) => details.put_to_index(author_id, event_id).await?,
            None => tracing::error!(
                "{}:{} Could not found event in the graph",
                author_id,
                event_id
            ),
        }
        Ok(())
    }

    /// Save new graph node
    pub async fn put_to_graph(&self) -> Result<OperationOutcome, DynError> {
        match queries::put::create_event(self) {
            Ok(query) => execute_graph_operation(query).await,
            Err(e) => Err(format!("QUERY: Error while creating the query: {e}").into()),
        }
    }

    pub async fn delete(author_id: &str, event_id: &str) -> Result<(), DynError> {
        // Delete from the graph database
        let query = queries::del::delete_event(author_id, event_id);
        exec_single_row(query).await?;

        // Delete from Redis cache
        Self::remove_from_index_multiple_json(&[&[author_id, event_id]]).await?;
        Ok(())
    }

    /// Stream events with optional filtering
    pub async fn stream(
        skip: usize,
        limit: usize,
        calendar: Option<String>,
        status: Option<String>,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<Vec<EventDetails>, DynError> {
        let query = queries::get::stream_events(skip, limit, calendar, status, start_date, end_date);
        let rows = crate::db::fetch_all_rows_from_graph(query).await?;
        let mut events = Vec::new();

        for row in rows {
            let event: EventDetails = row.get("event")?;
            events.push(event);
        }

        Ok(events)
    }
}
