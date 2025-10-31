use crate::routes::v0::endpoints::EVENT_ROUTE;
use crate::routes::AppState;
use axum::routing::get;
use axum::Router;
use utoipa::OpenApi;

mod view;

pub fn routes() -> Router<AppState> {
    Router::new().route(EVENT_ROUTE, get(view::event_view_handler))
}

#[derive(OpenApi)]
#[openapi()]
pub struct EventApiDoc;

impl EventApiDoc {
    pub fn merge_docs() -> utoipa::openapi::OpenApi {
        view::EventViewApiDoc::openapi()
    }
}
