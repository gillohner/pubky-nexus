use crate::db::{
    exec_single_row, execute_graph_operation, fetch_row_from_graph, queries, OperationOutcome,
    RedisOps,
};
use crate::types::DynError;
use chrono::Utc;
use pubky_app_specs::{event_uri_builder, PubkyAppEvent, PubkyId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Represents event data with full RFC 5545/7986/9073 fields
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct EventDetails {
    pub id: String,
    pub indexed_at: i64,
    pub author: String,
    pub uri: String,
    // Required RFC 5545 fields
    pub uid: String,
    pub dtstamp: String,
    pub dtstart: String,
    pub summary: String,
    // Optional RFC 5545 fields
    pub dtend: Option<String>,
    pub duration: Option<String>,
    pub rrule: Option<String>,
    pub rdate: Option<Vec<String>>,
    pub exdate: Option<Vec<String>>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub location: Option<String>,
    pub geo: Option<String>,
    pub organizer: Option<String>, // Serialized JSON
    pub url: Option<String>,
    pub categories: Option<Vec<String>>,
    pub class: Option<String>,
    pub priority: Option<i32>,
    pub sequence: Option<i32>,
    pub transp: Option<String>,
    pub attach: Option<Vec<String>>,
    pub attendee: Option<Vec<String>>,
    pub comment: Option<Vec<String>>,
    pub contact: Option<Vec<String>>,
    pub related_to: Option<Vec<String>>,
    pub request_status: Option<Vec<String>>,
    pub resources: Option<Vec<String>>,
    // RFC 7986 fields
    pub color: Option<String>,
    pub conference: Option<Vec<String>>, // Serialized JSON array
    pub image: Option<Vec<String>>,
    pub structured_locations: Option<String>, // Serialized JSON array
    pub styled_description: Option<String>,   // Serialized JSON
    // RFC 9073 fields
    pub participant_type: Option<Vec<String>>,
    pub resource_type: Option<Vec<String>>,
    pub structured_data: Option<String>,
    pub styled_description_param: Option<String>,
    // Pubky extensions
    pub x_pubky_recurrence_id: Option<String>,
    pub x_pubky_calendar_uri: Option<String>, // Reference to containing calendar
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
        // Serialize complex types to JSON strings for storage
        let organizer = homeserver_event
            .organizer
            .as_ref()
            .and_then(|o| serde_json::to_string(o).ok());
        let conference = homeserver_event
            .conference
            .as_ref()
            .and_then(|c| serde_json::to_string(c).ok())
            .map(|json| vec![json]);
        let structured_locations = homeserver_event
            .structured_locations
            .as_ref()
            .and_then(|sl| serde_json::to_string(sl).ok());
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
            dtstamp: homeserver_event.dtstamp.to_string(),
            dtstart: homeserver_event.dtstart.to_string(),
            summary: homeserver_event.summary,
            // Optional fields
            dtend: homeserver_event.dtend.map(|t| t.to_string()),
            duration: homeserver_event.duration,
            rrule: homeserver_event.rrule,
            rdate: homeserver_event.rdate,
            exdate: homeserver_event.exdate,
            description: homeserver_event.description,
            status: homeserver_event.status,
            location: homeserver_event.location,
            geo: homeserver_event.geo,
            organizer,
            url: homeserver_event.url,
            categories: homeserver_event.categories,
            class: None,
            priority: None,
            sequence: homeserver_event.sequence,
            transp: None,
            attach: None,
            attendee: None,
            comment: None,
            contact: None,
            related_to: None,
            request_status: None,
            resources: None,
            // RFC 7986
            color: None,
            conference,
            image: homeserver_event.image_uri.map(|uri| vec![uri]),
            structured_locations,
            styled_description,
            // RFC 9073
            participant_type: None,
            resource_type: None,
            structured_data: None,
            styled_description_param: None,
            // Pubky extensions
            x_pubky_recurrence_id: homeserver_event.recurrence_id.map(|id| id.to_string()),
            x_pubky_calendar_uri: homeserver_event.x_pubky_calendar_uris.and_then(|uris| uris.into_iter().next()),
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
