use crate::db::{
    exec_single_row, execute_graph_operation, fetch_row_from_graph, queries, OperationOutcome,
    RedisOps,
};
use crate::types::DynError;
use chrono::Utc;
use pubky_app_specs::{alarm_uri_builder, PubkyAppAlarm, PubkyId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Represents alarm/reminder data for an event
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct AlarmDetails {
    pub id: String,
    pub indexed_at: i64,
    pub author: String,
    pub uri: String,
    // Required fields
    pub action: String,
    pub trigger: String,
    pub x_pubky_target_uri: String,
    // Optional fields
    pub duration: Option<String>,
    pub repeat: Option<i32>,
    pub attach: Option<Vec<String>>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub attendees: Option<Vec<String>>,  // Note: plural, not singular
}

impl RedisOps for AlarmDetails {}

impl AlarmDetails {
    /// Retrieves alarm details by author ID and alarm ID, first trying Redis, then Neo4j
    pub async fn get_by_id(
        author_id: &str,
        alarm_id: &str,
    ) -> Result<Option<AlarmDetails>, DynError> {
        match Self::get_from_index(author_id, alarm_id).await? {
            Some(details) => Ok(Some(details)),
            None => {
                let graph_response = Self::get_from_graph(author_id, alarm_id).await?;
                if let Some(alarm_details) = graph_response {
                    alarm_details.put_to_index(author_id, alarm_id).await?;
                    return Ok(Some(alarm_details));
                }
                Ok(None)
            }
        }
    }

    pub async fn get_from_index(
        author_id: &str,
        alarm_id: &str,
    ) -> Result<Option<AlarmDetails>, DynError> {
        if let Some(alarm_details) =
            Self::try_from_index_json(&[author_id, alarm_id], None).await?
        {
            return Ok(Some(alarm_details));
        }
        Ok(None)
    }

    /// Retrieves the alarm fields from Neo4j
    pub async fn get_from_graph(
        author_id: &str,
        alarm_id: &str,
    ) -> Result<Option<AlarmDetails>, DynError> {
        let query = queries::get::get_alarm_by_id(author_id, alarm_id);
        let maybe_row = fetch_row_from_graph(query).await?;

        let Some(row) = maybe_row else {
            return Ok(None);
        };

        let alarm: AlarmDetails = row.get("details")?;
        Ok(Some(alarm))
    }

    pub async fn put_to_index(&self, author_id: &str, alarm_id: &str) -> Result<(), DynError> {
        self.put_index_json(&[author_id, alarm_id], None, None)
            .await?;
        Ok(())
    }

    pub async fn from_homeserver(
        homeserver_alarm: PubkyAppAlarm,
        author_id: &PubkyId,
        alarm_id: &String,
    ) -> Result<Self, DynError> {
        Ok(AlarmDetails {
            uri: alarm_uri_builder(author_id.to_string(), alarm_id.into()),
            id: alarm_id.clone(),
            indexed_at: Utc::now().timestamp_millis(),
            author: author_id.to_string(),
            action: homeserver_alarm.action,
            trigger: homeserver_alarm.trigger,
            x_pubky_target_uri: homeserver_alarm.x_pubky_target_uri,
            duration: homeserver_alarm.duration,
            repeat: homeserver_alarm.repeat,
            attach: homeserver_alarm.attach,
            description: homeserver_alarm.description,
            summary: homeserver_alarm.summary,
            attendees: homeserver_alarm.attendees,
        })
    }

    pub async fn reindex(author_id: &str, alarm_id: &str) -> Result<(), DynError> {
        match Self::get_from_graph(author_id, alarm_id).await? {
            Some(details) => details.put_to_index(author_id, alarm_id).await?,
            None => tracing::error!(
                "{}:{} Could not found alarm in the graph",
                author_id,
                alarm_id
            ),
        }
        Ok(())
    }

    /// Save new graph node
    pub async fn put_to_graph(&self) -> Result<OperationOutcome, DynError> {
        match queries::put::create_alarm(self) {
            Ok(query) => execute_graph_operation(query).await,
            Err(e) => Err(format!("QUERY: Error while creating the query: {e}").into()),
        }
    }

    pub async fn delete(author_id: &str, alarm_id: &str) -> Result<(), DynError> {
        // Delete from the graph database
        let query = queries::del::delete_alarm(author_id, alarm_id);
        exec_single_row(query).await?;

        // Delete from Redis cache
        Self::remove_from_index_multiple_json(&[&[author_id, alarm_id]]).await?;
        Ok(())
    }
}
