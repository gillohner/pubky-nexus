use crate::routes::v0::endpoints::CALENDAR_ROUTE;
use crate::{Error, Result};
use axum::extract::Path;
use axum::Json;
use nexus_common::models::calendar::CalendarDetails;
use tracing::info;
use utoipa::OpenApi;

#[utoipa::path(
    get,
    path = CALENDAR_ROUTE,
    description = "Calendar view",
    tag = "Calendar",
    params(
        ("author_id" = String, Path, description = "Author Pubky ID"),
        ("calendar_id" = String, Path, description = "Calendar Crockford32 ID")
    ),
    responses(
        (status = 200, description = "Calendar", body = CalendarDetails),
        (status = 404, description = "Calendar not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn calendar_view_handler(
    Path((author_id, calendar_id)): Path<(String, String)>,
) -> Result<Json<CalendarDetails>> {
    info!(
        "GET {CALENDAR_ROUTE} author_id:{}, calendar_id:{}",
        author_id, calendar_id
    );

    match CalendarDetails::get_by_id(&author_id, &calendar_id).await {
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
    components(schemas(CalendarDetails))
)]
pub struct CalendarViewApiDoc;
