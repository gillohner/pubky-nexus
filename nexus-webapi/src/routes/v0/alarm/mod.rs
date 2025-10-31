use crate::routes::v0::endpoints::ALARM_ROUTE;
use crate::routes::AppState;
use axum::routing::get;
use axum::Router;
use utoipa::OpenApi;

mod view;

pub fn routes() -> Router<AppState> {
    Router::new().route(ALARM_ROUTE, get(view::alarm_view_handler))
}

#[derive(OpenApi)]
#[openapi()]
pub struct AlarmApiDoc;

impl AlarmApiDoc {
    pub fn merge_docs() -> utoipa::openapi::OpenApi {
        view::AlarmViewApiDoc::openapi()
    }
}
