use crate::routes::v0::endpoints::ATTENDEE_ROUTE;
use crate::routes::AppState;
use axum::routing::get;
use axum::Router;
use utoipa::OpenApi;

mod view;

pub fn routes() -> Router<AppState> {
    Router::new().route(ATTENDEE_ROUTE, get(view::attendee_view_handler))
}

#[derive(OpenApi)]
#[openapi()]
pub struct AttendeeApiDoc;

impl AttendeeApiDoc {
    pub fn merge_docs() -> utoipa::openapi::OpenApi {
        view::AttendeeViewApiDoc::openapi()
    }
}
