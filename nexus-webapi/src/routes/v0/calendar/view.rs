use crate::routes::v0::endpoints::CALENDAR_ROUTE;
use crate::routes::v0::CalendarQuery;
use crate::{Error, Result};
use axum::extract::{Path, Query};
use axum::Json;
use nexus_common::models::calendar::{CalendarDetails, CalendarView};
use nexus_common::models::tag::calendar::TagCalendar;
use nexus_common::models::tag::TagDetails;
use tracing::info;
use utoipa::OpenApi;

#[utoipa::path(
    get,
    path = CALENDAR_ROUTE,
    description = "Calendar view with tags and events",
    tag = "Calendar",
    params(
        ("author_id" = String, Path, description = "Author Pubky ID"),
        ("calendar_id" = String, Path, description = "Calendar Crockford32 ID"),
        ("viewer_id" = Option<String>, Query, description = "Viewer Pubky ID"),
        ("limit_tags" = Option<usize>, Query, description = "Upper limit on the number of tags for the calendar"),
        ("limit_taggers" = Option<usize>, Query, description = "Upper limit on the number of taggers per tag"),
        ("limit_events" = Option<usize>, Query, description = "Upper limit on the number of event URIs to return (default: 100)")
    ),
    responses(
        (status = 200, description = "Calendar", body = CalendarView),
        (status = 404, description = "Calendar not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn calendar_view_handler(
    Path((author_id, calendar_id)): Path<(String, String)>,
    Query(query): Query<CalendarQuery>,
) -> Result<Json<CalendarView>> {
    info!(
        "GET {CALENDAR_ROUTE} author_id:{}, calendar_id:{}, viewer_id:{}, limit_tags:{:?}, limit_taggers:{:?}, limit_events:{:?}",
        author_id,
        calendar_id,
        query.viewer_id.clone().unwrap_or_default(),
        query.limit_tags,
        query.limit_taggers,
        query.limit_events
    );

    match CalendarView::get_by_id_with_events(
        &author_id,
        &calendar_id,
        query.viewer_id.as_deref(),
        query.limit_tags,
        query.limit_taggers,
        query.limit_events,
    )
    .await
    {
        Ok(Some(calendar)) => Ok(Json(calendar)),
        Ok(None) => Err(Error::CalendarNotFound {
            author_id,
            calendar_id,
        }),
        Err(source) => Err(Error::InternalServerError { source }),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(calendar_view_handler),
    components(schemas(CalendarView, CalendarDetails, TagCalendar, TagDetails))
)]
pub struct CalendarViewApiDoc;
