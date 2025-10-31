use crate::routes::v0::endpoints::{
    STREAM_CALENDARS_ROUTE, STREAM_EVENTS_ROUTE, STREAM_POSTS_BY_IDS_ROUTE, STREAM_POSTS_ROUTE,
    STREAM_USERS_BY_IDS_ROUTE, STREAM_USERS_ROUTE, STREAM_USERS_USERNAME_SEARCH_ROUTE,
};
use crate::routes::AppState;

use axum::routing::{get, post};
use axum::Router;
use utoipa::OpenApi;

mod calendars;
mod events;
mod posts;
mod users;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(STREAM_USERS_ROUTE, get(users::stream_users_handler))
        .route(
            STREAM_USERS_USERNAME_SEARCH_ROUTE,
            get(users::stream_username_search_handler),
        )
        .route(STREAM_POSTS_ROUTE, get(posts::stream_posts_handler))
        .route(
            STREAM_USERS_BY_IDS_ROUTE,
            post(users::stream_users_by_ids_handler),
        )
        .route(
            STREAM_POSTS_BY_IDS_ROUTE,
            post(posts::stream_posts_by_ids_handler),
        )
        .route(
            STREAM_CALENDARS_ROUTE,
            get(calendars::stream_calendars_handler),
        )
        .route(STREAM_EVENTS_ROUTE, get(events::stream_events_handler))
}

#[derive(OpenApi)]
#[openapi()]
pub struct StreamApiDoc;

impl StreamApiDoc {
    pub fn merge_docs() -> utoipa::openapi::OpenApi {
        let mut combined = users::StreamUsersApiDocs::openapi();
        combined.merge(posts::StreamPostsApiDocs::openapi());
        combined.merge(calendars::StreamCalendarsApiDocs::openapi());
        combined.merge(events::StreamEventsApiDocs::openapi());
        combined
    }
}
