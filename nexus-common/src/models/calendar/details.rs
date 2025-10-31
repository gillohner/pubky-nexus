use crate::db::{
    exec_single_row, execute_graph_operation, fetch_row_from_graph, queries, OperationOutcome,
    RedisOps,
};
use crate::types::DynError;
use chrono::Utc;
use pubky_app_specs::{calendar_uri_builder, PubkyAppCalendar, PubkyId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Represents calendar data with name, timezone, color and metadata
#[derive(Serialize, Deserialize, ToSchema, Default, Debug)]
pub struct CalendarDetails {
    pub id: String,
    pub indexed_at: i64,
    pub author: String,
    pub uri: String,
    pub name: String,
    pub timezone: String,
    pub color: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub image_uri: Option<String>,
    pub x_pubky_admins: Option<Vec<String>>,
    pub created: Option<i64>,
}

impl RedisOps for CalendarDetails {}

impl CalendarDetails {
    /// Retrieves calendar details by author ID and calendar ID, first trying Redis, then Neo4j
    pub async fn get_by_id(
        author_id: &str,
        calendar_id: &str,
    ) -> Result<Option<CalendarDetails>, DynError> {
        match Self::get_from_index(author_id, calendar_id).await? {
            Some(details) => Ok(Some(details)),
            None => {
                let graph_response = Self::get_from_graph(author_id, calendar_id).await?;
                if let Some(calendar_details) = graph_response {
                    calendar_details
                        .put_to_index(author_id, calendar_id)
                        .await?;
                    return Ok(Some(calendar_details));
                }
                Ok(None)
            }
        }
    }

    pub async fn get_from_index(
        author_id: &str,
        calendar_id: &str,
    ) -> Result<Option<CalendarDetails>, DynError> {
        if let Some(calendar_details) =
            Self::try_from_index_json(&[author_id, calendar_id], None).await?
        {
            return Ok(Some(calendar_details));
        }
        Ok(None)
    }

    /// Retrieves the calendar fields from Neo4j
    pub async fn get_from_graph(
        author_id: &str,
        calendar_id: &str,
    ) -> Result<Option<CalendarDetails>, DynError> {
        let query = queries::get::get_calendar_by_id(author_id, calendar_id);
        let maybe_row = fetch_row_from_graph(query).await?;

        let Some(row) = maybe_row else {
            return Ok(None);
        };

        let calendar: CalendarDetails = row.get("details")?;
        Ok(Some(calendar))
    }

    pub async fn put_to_index(
        &self,
        author_id: &str,
        calendar_id: &str,
    ) -> Result<(), DynError> {
        self.put_index_json(&[author_id, calendar_id], None, None)
            .await?;
        // TODO: Add to calendar streams when implementing stream endpoints
        Ok(())
    }

    pub async fn from_homeserver(
        homeserver_calendar: PubkyAppCalendar,
        author_id: &PubkyId,
        calendar_id: &String,
    ) -> Result<Self, DynError> {
        Ok(CalendarDetails {
            uri: calendar_uri_builder(author_id.to_string(), calendar_id.into()),
            name: homeserver_calendar.name,
            timezone: homeserver_calendar.timezone,
            color: homeserver_calendar.color,
            description: homeserver_calendar.description,
            url: homeserver_calendar.url,
            image_uri: homeserver_calendar.image_uri,
            x_pubky_admins: homeserver_calendar.x_pubky_admins,
            created: homeserver_calendar.created,
            id: calendar_id.clone(),
            indexed_at: Utc::now().timestamp_millis(),
            author: author_id.to_string(),
        })
    }

    pub async fn reindex(author_id: &str, calendar_id: &str) -> Result<(), DynError> {
        match Self::get_from_graph(author_id, calendar_id).await? {
            Some(details) => details.put_to_index(author_id, calendar_id).await?,
            None => tracing::error!(
                "{}:{} Could not found calendar in the graph",
                author_id,
                calendar_id
            ),
        }
        Ok(())
    }

    /// Save new graph node
    pub async fn put_to_graph(&self) -> Result<OperationOutcome, DynError> {
        match queries::put::create_calendar(self) {
            Ok(query) => execute_graph_operation(query).await,
            Err(e) => Err(format!("QUERY: Error while creating the query: {e}").into()),
        }
    }

    pub async fn delete(author_id: &str, calendar_id: &str) -> Result<(), DynError> {
        // Delete from the graph database
        let query = queries::del::delete_calendar(author_id, calendar_id);
        exec_single_row(query).await?;

        // Delete from Redis cache
        Self::remove_from_index_multiple_json(&[&[author_id, calendar_id]]).await?;
        Ok(())
    }

    /// Stream calendars with optional filtering
    pub async fn stream(
        skip: usize,
        limit: usize,
        admin: Option<String>,
    ) -> Result<Vec<CalendarDetails>, DynError> {
        let query = queries::get::stream_calendars(skip, limit, admin);
        let rows = crate::db::fetch_all_rows_from_graph(query).await?;
        let mut calendars = Vec::new();

        for row in rows {
            let calendar: CalendarDetails = row.get("calendar")?;
            calendars.push(calendar);
        }

        Ok(calendars)
    }
}
