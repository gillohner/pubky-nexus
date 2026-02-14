use crate::routes::v0::endpoints::STREAM_CALENDARS_ROUTE;
use crate::{Error, Result as AppResult};
use axum::{extract::Query, Json};
use nexus_common::models::calendar::{CalendarDetails, CalendarStreamItem};
use nexus_common::types::Pagination;
use serde::Deserialize;
use tracing::info;
use utoipa::{OpenApi, ToSchema};

pub type CalendarStream = Vec<CalendarStreamItem>;

#[derive(Deserialize, Debug, ToSchema)]
pub struct CalendarStreamQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    pub admin: Option<String>,
    pub author: Option<String>,
}

impl CalendarStreamQuery {
    pub fn initialize_defaults(&mut self) {
        self.pagination.skip.get_or_insert(0);
        self.pagination.limit = Some(self.pagination.limit.unwrap_or(10).min(100));
    }
}

#[utoipa::path(
    get,
    path = STREAM_CALENDARS_ROUTE,
    tag = "Stream",
    params(
        ("limit" = Option<usize>, Query, description = "Number of results to return (default: 50, max: 100)"),
        ("skip" = Option<usize>, Query, description = "Number of results to skip (default: 0)"),
        ("admin" = Option<String>, Query, description = "Filter calendars where user is admin"),
        ("author" = Option<String>, Query, description = "Filter calendars by author/creator user ID"),
    ),
    responses(
        (status = 200, description = "Calendar stream", body = CalendarStream),
        (status = 404, description = "Calendars not found"),
        (status = 500, description = "Internal server error")
    ),
    description = "Stream Calendars\n\nRetrieve a list of calendars with optional filtering. Each calendar includes inline tag information."
)]
pub async fn stream_calendars_handler(
    Query(mut query): Query<CalendarStreamQuery>,
) -> AppResult<Json<CalendarStream>> {
    query.initialize_defaults();

    let skip = query.pagination.skip.unwrap_or(0);
    let limit = query.pagination.limit.unwrap_or(10);

    info!(
        "GET {STREAM_CALENDARS_ROUTE} skip:{:?}, limit:{:?}, admin:{:?}, author:{:?}",
        skip, limit, query.admin, query.author
    );

    match CalendarDetails::stream(skip, limit, query.admin, query.author).await {
        Ok(calendars) => Ok(Json(calendars)),
        Err(source) => Err(Error::InternalServerError { source }),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(stream_calendars_handler),
    components(schemas(CalendarStreamQuery))
)]
pub struct StreamCalendarsApiDocs;
