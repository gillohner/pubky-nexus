use crate::routes::v0::endpoints::CALENDAR_ROUTE;
use crate::routes::AppState;
use axum::routing::get;
use axum::Router;
use utoipa::OpenApi;

mod view;

pub fn routes() -> Router<AppState> {
    Router::new().route(CALENDAR_ROUTE, get(view::calendar_view_handler))
}

#[derive(OpenApi)]
#[openapi()]
pub struct CalendarApiDoc;

impl CalendarApiDoc {
    pub fn merge_docs() -> utoipa::openapi::OpenApi {
        view::CalendarViewApiDoc::openapi()
    }
}
