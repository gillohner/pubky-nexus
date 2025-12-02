use axum::Router;
use utoipa::OpenApi;

pub mod attendee;
pub mod bootstrap;
pub mod calendar;
pub mod endpoints;
pub mod event;
pub mod file;
pub mod info;
pub mod notification;
pub mod post;
pub mod search;
pub mod stream;
pub mod tag;
mod types;
pub mod user;
pub mod utils;

pub use types::{TaggersInfoResponse, TagsQuery};

use super::AppState;

pub fn routes(app_state: AppState) -> Router<AppState> {
    let routes_info = info::routes(app_state);
    let routes_post = post::routes();
    let route_user = user::routes();
    let route_stream = stream::routes();
    let route_search = search::routes();
    let route_file = file::routes();
    let route_tag = tag::routes();
    let route_notification = notification::routes();
    let route_bootstrap = bootstrap::routes();
    let route_calendar = calendar::routes();
    let route_event = event::routes();
    let route_attendee = attendee::routes();

    routes_post
        .merge(routes_info)
        .merge(route_user)
        .merge(route_stream)
        .merge(route_search)
        .merge(route_file)
        .merge(route_tag)
        .merge(route_notification)
        .merge(route_bootstrap)
        .merge(route_calendar)
        .merge(route_event)
        .merge(route_attendee)
}

#[derive(OpenApi)]
#[openapi()]
pub struct ApiDoc;

impl ApiDoc {
    pub fn merge_docs() -> utoipa::openapi::OpenApi {
        let mut combined = post::PostApiDoc::merge_docs();
        combined.merge(bootstrap::BootstrapApiDoc::openapi());
        combined.merge(info::InfoApiDoc::openapi());
        combined.merge(user::UserApiDoc::merge_docs());
        combined.merge(stream::StreamApiDoc::merge_docs());
        combined.merge(search::SearchApiDoc::merge_docs());
        combined.merge(search::SearchApiDoc::merge_docs());
        combined.merge(file::FileApiDoc::merge_docs());
        combined.merge(tag::TagApiDoc::merge_docs());
        combined.merge(notification::NotificationApiDoc::merge_docs());
        combined.merge(calendar::CalendarApiDoc::merge_docs());
        combined.merge(event::EventApiDoc::merge_docs());
        combined.merge(attendee::AttendeeApiDoc::merge_docs());
        combined
    }
}
