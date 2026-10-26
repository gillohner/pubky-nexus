use super::endpoints::APP_POST_CHANGES_ROUTE;
use crate::models::BoundedLimit;
use crate::routes::{AppState, Query};
use crate::Result;
use axum::http::{HeaderMap, HeaderValue};
use axum::{routing::get, Json, Router};
use nexus_common::models::event::app_post_changes::{AppPostChange, AppPostChangesPage};
use serde::Deserialize;
use utoipa::OpenApi;

#[derive(Deserialize)]
pub struct AppPostChangesQuery {
    cursor: Option<u64>,
    limit: Option<BoundedLimit<500, 1000>>,
}

#[utoipa::path(
    get,
    path = APP_POST_CHANGES_ROUTE,
    tag = "App posts",
    description = "Content-free app-post and selected social post changes, independent of /v0/resources/changes. Social selection uses watcher.post_feed.social_kinds (default event and rsvp); those posts retain social indexing and namespace social. Delivery contract: at least once, in append order; retries may duplicate puts and must be harmless. Logical identity is (author, post). PUT upserts its version in that post's version set; fetch pubky://{author}{version}. DEL with a version removes only that file; DEL with version null removes the whole post. N3 deletes the logical graph node on any version DEL, but the feed reports the actual deleted file and does not infer deletion of other stored versions. post_kind belongs to namespace, not the resource reference vocabulary. at is Unix milliseconds. Read-only: never seeds or fetches content. Persist cursor and epoch together. On epoch change or requested cursor greater than x-events-length, restart at 0 and rebuild state. Page, epoch and length are one atomic snapshot. x-events-epoch is absent until the first append or seed; retain the prior epoch when absent. Reseed preserves live appends; snapshot duplicates are harmless.",
    params(
        ("cursor" = Option<u64>, Query, description = "Zero-based list offset; default 0"),
        ("limit" = Option<BoundedLimit<500, 1000>>, Query, description = "Page size, 1–1000; default 500")
    ),
    responses(
        (status = 200, description = "App post changes and next cursor", body = AppPostChangesPage,
            headers(
                ("x-events-epoch" = String, description = "Opaque feed generation; absent before initialization. Changes on reseed or a new feed."),
                ("x-events-length" = u64, description = "List length from the same atomic snapshot as the page")
            )),
        (status = 400, description = "Invalid cursor or limit"),
        (status = 429, description = "Rate limit exceeded", headers(("Retry-After" = u64, description = "Seconds until retry"))),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn app_post_changes_handler(
    Query(query): Query<AppPostChangesQuery>,
) -> Result<(HeaderMap, Json<AppPostChangesPage>)> {
    let (page, epoch, length) = AppPostChange::get_from_index(
        query.cursor.unwrap_or(0),
        query.limit.map_or(500, |limit| limit.value()),
    )
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert("x-events-length", HeaderValue::from(length));
    if let Some(epoch) = epoch {
        headers.insert("x-events-epoch", HeaderValue::from_str(&epoch)?);
    }
    Ok((headers, Json(page)))
}

pub fn routes() -> Router<AppState> {
    Router::new().route(APP_POST_CHANGES_ROUTE, get(app_post_changes_handler))
}

#[derive(OpenApi)]
#[openapi(paths(app_post_changes_handler))]
pub struct AppPostChangesApiDoc;
