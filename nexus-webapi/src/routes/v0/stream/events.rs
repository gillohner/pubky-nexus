use crate::routes::v0::endpoints::STREAM_EVENTS_ROUTE;
use crate::utils::serde_helpers::deserialize_comma_separated;
use crate::{Error, Result as AppResult};
use axum::{extract::Query, Json};
use nexus_common::models::event::{EventDetails, EventStreamItem};
use nexus_common::types::Pagination;
use serde::Deserialize;
use tracing::info;
use utoipa::{OpenApi, ToSchema};

pub type EventStream = Vec<EventStreamItem>;

#[derive(Deserialize, Debug, ToSchema)]
pub struct EventStreamQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    #[serde(default, deserialize_with = "deserialize_comma_separated")]
    pub tags: Option<Vec<String>>,
    pub calendar: Option<String>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_comma_separated")]
    pub authors: Option<Vec<String>>,
}

impl EventStreamQuery {
    pub fn initialize_defaults(&mut self) {
        self.pagination.skip.get_or_insert(0);
        self.pagination.limit = Some(self.pagination.limit.unwrap_or(10).min(100));
    }
}

#[utoipa::path(
    get,
    path = STREAM_EVENTS_ROUTE,
    tag = "Stream",
    params(
        ("limit" = Option<usize>, Query, description = "Number of results to return (default: 50, max: 100)"),
        ("skip" = Option<usize>, Query, description = "Number of results to skip (default: 0)"),
        ("tags" = Option<Vec<String>>, Query, description = "Filter by a list of comma-separated tags. E.g., `&tags=meetup,workshop,conference`. Only events matching at least one of the tags will be returned."),
        ("calendar" = Option<String>, Query, description = "Filter events by calendar URI or ID"),
        ("start_date" = Option<i64>, Query, description = "Filter events starting after this date (Unix microseconds)"),
        ("end_date" = Option<i64>, Query, description = "Filter events starting before this date (Unix microseconds)"),
        ("status" = Option<String>, Query, description = "Filter by event status (CONFIRMED, TENTATIVE, CANCELLED)"),
        ("authors" = Option<Vec<String>>, Query, description = "Filter events by one or more author IDs (comma-separated)"),
    ),
    responses(
        (status = 200, description = "Event stream", body = EventStream),
        (status = 404, description = "Events not found"),
        (status = 500, description = "Internal server error")
    ),
    description = "Stream Events\n\nRetrieve a list of events with optional filtering. Each event includes inline tag information."
)]
pub async fn stream_events_handler(
    Query(mut query): Query<EventStreamQuery>,
) -> AppResult<Json<EventStream>> {
    query.initialize_defaults();

    let skip = query.pagination.skip.unwrap_or(0);
    let limit = query.pagination.limit.unwrap_or(10);

    info!(
        "GET {STREAM_EVENTS_ROUTE} skip:{:?}, limit:{:?}, tags:{:?}, calendar:{:?}, start_date:{:?}, end_date:{:?}, status:{:?}, authors:{:?}",
        skip, limit, query.tags, query.calendar,
        query.start_date, query.end_date, query.status, query.authors
    );

    match EventDetails::stream(skip, limit, query.calendar, query.status, query.start_date, query.end_date, query.authors, query.tags).await {
        Ok(events) => Ok(Json(events)),
        Err(source) => Err(Error::InternalServerError { source }),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(stream_events_handler),
    components(schemas(EventStreamQuery))
)]
pub struct StreamEventsApiDocs;
