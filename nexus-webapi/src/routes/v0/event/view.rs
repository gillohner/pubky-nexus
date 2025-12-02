use crate::routes::v0::endpoints::EVENT_ROUTE;
use crate::routes::v0::EventQuery;
use crate::{Error, Result};
use axum::extract::{Path, Query};
use axum::Json;
use nexus_common::models::attendee::AttendeeDetails;
use nexus_common::models::event::{EventDetails, EventView};
use nexus_common::models::tag::event::TagEvent;
use nexus_common::models::tag::TagDetails;
use tracing::info;
use utoipa::OpenApi;

#[utoipa::path(
    get,
    path = EVENT_ROUTE,
    description = "Event view with tags and attendees",
    tag = "Event",
    params(
        ("author_id" = String, Path, description = "Author Pubky ID"),
        ("event_id" = String, Path, description = "Event Crockford32 ID"),
        ("viewer_id" = Option<String>, Query, description = "Viewer Pubky ID"),
        ("limit_tags" = Option<usize>, Query, description = "Upper limit on the number of tags for the event"),
        ("limit_taggers" = Option<usize>, Query, description = "Upper limit on the number of taggers per tag"),
        ("limit_attendees" = Option<usize>, Query, description = "Upper limit on the number of attendees")
    ),
    responses(
        (status = 200, description = "Event", body = EventView),
        (status = 404, description = "Event not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn event_view_handler(
    Path((author_id, event_id)): Path<(String, String)>,
    Query(query): Query<EventQuery>,
) -> Result<Json<EventView>> {
    info!(
        "GET {EVENT_ROUTE} author_id:{}, event_id:{}, viewer_id:{}, limit_tags:{:?}, limit_taggers:{:?}, limit_attendees:{:?}",
        author_id,
        event_id,
        query.viewer_id.clone().unwrap_or_default(),
        query.limit_tags,
        query.limit_taggers,
        query.limit_attendees
    );

    match EventView::get_by_id(
        &author_id,
        &event_id,
        query.viewer_id.as_deref(),
        query.limit_tags,
        query.limit_taggers,
        query.limit_attendees,
    )
    .await
    {
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
    components(schemas(EventView, EventDetails, TagEvent, TagDetails, AttendeeDetails))
)]
pub struct EventViewApiDoc;
