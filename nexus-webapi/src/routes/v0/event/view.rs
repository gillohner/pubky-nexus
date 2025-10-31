use crate::routes::v0::endpoints::EVENT_ROUTE;
use crate::{Error, Result};
use axum::extract::Path;
use axum::Json;
use nexus_common::models::event::EventDetails;
use tracing::info;
use utoipa::OpenApi;

#[utoipa::path(
    get,
    path = EVENT_ROUTE,
    description = "Event view",
    tag = "Event",
    params(
        ("author_id" = String, Path, description = "Author Pubky ID"),
        ("event_id" = String, Path, description = "Event Crockford32 ID")
    ),
    responses(
        (status = 200, description = "Event", body = EventDetails),
        (status = 404, description = "Event not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn event_view_handler(
    Path((author_id, event_id)): Path<(String, String)>,
) -> Result<Json<EventDetails>> {
    info!(
        "GET {EVENT_ROUTE} author_id:{}, event_id:{}",
        author_id, event_id
    );

    match EventDetails::get_by_id(&author_id, &event_id).await {
        Ok(Some(event)) => Ok(Json(event)),
        Ok(None) => Err(Error::EventNotFound {
            author_id,
            event_id,
        }),
        Err(source) => Err(Error::InternalServerError { source }),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(event_view_handler),
    components(schemas(EventDetails))
)]
pub struct EventViewApiDoc;
