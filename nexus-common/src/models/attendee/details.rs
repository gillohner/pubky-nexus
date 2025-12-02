use crate::db::{
    exec_single_row, execute_graph_operation, fetch_row_from_graph, queries, OperationOutcome,
    RedisOps,
};
use crate::types::DynError;
use chrono::Utc;
use pubky_app_specs::{attendee_uri_builder, PubkyAppAttendee, PubkyId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Represents attendee/RSVP data for an event
/// Simplified to match PubkyAppAttendee spec
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct AttendeeDetails {
    pub id: String,
    pub indexed_at: i64,
    pub author: String,
    pub uri: String,
    // Required fields
    pub partstat: String,
    pub x_pubky_event_uri: String,
    // Optional fields
    pub created_at: i64,
    pub last_modified: Option<i64>,
    pub recurrence_id: Option<i64>,
}

impl RedisOps for AttendeeDetails {}

impl AttendeeDetails {
    /// Retrieves attendee details by author ID and attendee ID, first trying Redis, then Neo4j
    pub async fn get_by_id(
        author_id: &str,
        attendee_id: &str,
    ) -> Result<Option<AttendeeDetails>, DynError> {
        match Self::get_from_index(author_id, attendee_id).await? {
            Some(details) => Ok(Some(details)),
            None => {
                let graph_response = Self::get_from_graph(author_id, attendee_id).await?;
                if let Some(attendee_details) = graph_response {
                    attendee_details
                        .put_to_index(author_id, attendee_id)
                        .await?;
                    return Ok(Some(attendee_details));
                }
                Ok(None)
            }
        }
    }

    pub async fn get_from_index(
        author_id: &str,
        attendee_id: &str,
    ) -> Result<Option<AttendeeDetails>, DynError> {
        if let Some(attendee_details) =
            Self::try_from_index_json(&[author_id, attendee_id], None).await?
        {
            return Ok(Some(attendee_details));
        }
        Ok(None)
    }

    /// Retrieves the attendee fields from Neo4j
    pub async fn get_from_graph(
        author_id: &str,
        attendee_id: &str,
    ) -> Result<Option<AttendeeDetails>, DynError> {
        let query = queries::get::get_attendee_by_id(author_id, attendee_id);
        let maybe_row = fetch_row_from_graph(query).await?;

        let Some(row) = maybe_row else {
            return Ok(None);
        };

        let attendee: AttendeeDetails = row.get("details")?;
        Ok(Some(attendee))
    }

    pub async fn put_to_index(
        &self,
        author_id: &str,
        attendee_id: &str,
    ) -> Result<(), DynError> {
        self.put_index_json(&[author_id, attendee_id], None, None)
            .await?;
        Ok(())
    }

    pub async fn from_homeserver(
        homeserver_attendee: PubkyAppAttendee,
        author_id: &PubkyId,
        attendee_id: &String,
    ) -> Result<Self, DynError> {
        Ok(AttendeeDetails {
            uri: attendee_uri_builder(author_id.to_string(), attendee_id.into()),
            id: attendee_id.clone(),
            indexed_at: Utc::now().timestamp_millis(),
            author: author_id.to_string(),
            partstat: homeserver_attendee.partstat,
            x_pubky_event_uri: homeserver_attendee.x_pubky_event_uri,
            created_at: homeserver_attendee.created_at,
            last_modified: homeserver_attendee.last_modified,
            recurrence_id: homeserver_attendee.recurrence_id,
        })
    }

    pub async fn reindex(author_id: &str, attendee_id: &str) -> Result<(), DynError> {
        match Self::get_from_graph(author_id, attendee_id).await? {
            Some(details) => details.put_to_index(author_id, attendee_id).await?,
            None => tracing::error!(
                "{}:{} Could not found attendee in the graph",
                author_id,
                attendee_id
            ),
        }
        Ok(())
    }

    /// Save new graph node
    pub async fn put_to_graph(&self) -> Result<OperationOutcome, DynError> {
        match queries::put::create_attendee(self) {
            Ok(query) => execute_graph_operation(query).await,
            Err(e) => Err(format!("QUERY: Error while creating the query: {e}").into()),
        }
    }

    pub async fn delete(author_id: &str, attendee_id: &str) -> Result<(), DynError> {
        // Delete from the graph database
        let query = queries::del::delete_attendee(author_id, attendee_id);
        exec_single_row(query).await?;

        // Delete from Redis cache
        Self::remove_from_index_multiple_json(&[&[author_id, attendee_id]]).await?;
        Ok(())
    }
}
