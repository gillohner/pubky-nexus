use crate::routes::v0::endpoints::ATTENDEE_ROUTE;
use crate::{Error, Result};
use axum::extract::Path;
use axum::Json;
use nexus_common::models::attendee::AttendeeDetails;
use tracing::info;
use utoipa::OpenApi;

#[utoipa::path(
    get,
    path = ATTENDEE_ROUTE,
    description = "Attendee view",
    tag = "Attendee",
    params(
        ("author_id" = String, Path, description = "Author Pubky ID"),
        ("attendee_id" = String, Path, description = "Attendee Crockford32 ID")
    ),
    responses(
        (status = 200, description = "Attendee", body = AttendeeDetails),
        (status = 404, description = "Attendee not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn attendee_view_handler(
    Path((author_id, attendee_id)): Path<(String, String)>,
) -> Result<Json<AttendeeDetails>> {
    info!(
        "GET {ATTENDEE_ROUTE} author_id:{}, attendee_id:{}",
        author_id, attendee_id
    );

    match AttendeeDetails::get_by_id(&author_id, &attendee_id).await {
        Ok(Some(attendee)) => Ok(Json(attendee)),
        Ok(None) => Err(Error::AttendeeNotFound {
            author_id,
            attendee_id,
        }),
        Err(source) => Err(Error::InternalServerError { source }),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(attendee_view_handler),
    components(schemas(AttendeeDetails))
)]
pub struct AttendeeViewApiDoc;
