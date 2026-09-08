//! Private replication endpoints. Historical payloads must never be a public API.
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, Request},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use nexus_common::models::post_projection::{
    self as projection, ChangesPage, ChangesRequest, Checkpoint, InventoryPage, InventoryRequest,
    ProjectionError,
};
use serde::Deserialize;
use utoipa::OpenApi;

use super::endpoints::{
    POST_PROJECTION_CHANGES_ROUTE, POST_PROJECTION_HEAD_ROUTE, POST_PROJECTION_INVENTORY_ROUTE,
};
use crate::{
    models::{BoundedLimit, PostKinds},
    routes::{AppState, Query},
    Error,
};

#[derive(Deserialize)]
pub struct InventoryQuery {
    epoch: String,
    since: String,
    after: Option<String>,
    kinds: Option<PostKinds>,
    limit: Option<BoundedLimit<50, 100>>,
}

#[derive(Deserialize)]
pub struct ChangesQuery {
    epoch: String,
    after: String,
    through: Option<String>,
    limit: Option<BoundedLimit<50, 100>>,
}

#[utoipa::path(get, path = POST_PROJECTION_HEAD_ROUTE, tag = "Private projection",
    responses((status = 200, body = Checkpoint), (status = 403, description = "Missing or invalid server sync token")))]
pub async fn get_head() -> Result<axum::Json<Checkpoint>, Error> {
    projection::head().await.map(axum::Json).map_err(map_error)
}

#[utoipa::path(get, path = POST_PROJECTION_INVENTORY_ROUTE, tag = "Private projection",
    params(("epoch" = String, Query), ("since" = String, Query), ("after" = Option<String>, Query),
           ("kinds" = Option<String>, Query), ("limit" = Option<usize>, Query)),
    responses((status = 200, body = InventoryPage), (status = 403, description = "Missing or invalid server sync token"), (status = 410, description = "Restart inventory; epoch or retained interval changed")))]
pub async fn get_inventory(
    Query(query): Query<InventoryQuery>,
) -> Result<axum::Json<InventoryPage>, Error> {
    if query
        .after
        .as_ref()
        .is_some_and(|cursor| cursor.len() > 128)
    {
        return Err(Error::invalid_input("Inventory cursor exceeds 128 bytes"));
    }
    let kinds: Vec<String> = query
        .kinds
        .map(|kinds| kinds.0.into_iter().map(|kind| kind.to_string()).collect())
        .unwrap_or_default();
    let request = InventoryRequest {
        epoch: &query.epoch,
        since: projection::parse_revision(&query.since).map_err(map_error)?,
        after: query.after.as_deref(),
        kinds: &kinds,
        limit: query.limit.as_ref().map_or(50, |limit| limit.value()),
    };
    projection::inventory(request)
        .await
        .map(axum::Json)
        .map_err(map_error)
}

#[utoipa::path(get, path = POST_PROJECTION_CHANGES_ROUTE, tag = "Private projection",
    params(("epoch" = String, Query), ("after" = String, Query), ("through" = Option<String>, Query), ("limit" = Option<usize>, Query)),
    responses((status = 200, body = ChangesPage), (status = 403, description = "Missing or invalid server sync token"), (status = 410, description = "Restart inventory; epoch or retained interval changed")))]
pub async fn get_changes(
    Query(query): Query<ChangesQuery>,
) -> Result<axum::Json<ChangesPage>, Error> {
    let request = ChangesRequest {
        epoch: &query.epoch,
        after: projection::parse_revision(&query.after).map_err(map_error)?,
        through: query
            .through
            .as_deref()
            .map(projection::parse_revision)
            .transpose()
            .map_err(map_error)?,
        limit: query.limit.as_ref().map_or(50, |limit| limit.value()),
    };
    projection::changes(request)
        .await
        .map(axum::Json)
        .map_err(map_error)
}

fn map_error(error: ProjectionError) -> Error {
    match error {
        ProjectionError::ResetRequired(message) => Error::ProjectionResetRequired {
            message: message.into(),
        },
        ProjectionError::InvalidInput(message) => Error::invalid_input(message),
        ProjectionError::Graph(source) => Error::InternalServerError {
            source: Box::new(source),
        },
    }
}

pub fn routes() -> Router<AppState> {
    let token = std::env::var("NEXUS_PROJECTION_TOKEN")
        .ok()
        .map(Arc::<str>::from);
    Router::new()
        .route(POST_PROJECTION_HEAD_ROUTE, get(get_head))
        .route(POST_PROJECTION_INVENTORY_ROUTE, get(get_inventory))
        .route(POST_PROJECTION_CHANGES_ROUTE, get(get_changes))
        .layer(middleware::from_fn_with_state(token, authorize))
}

async fn authorize(
    State(token): State<Option<Arc<str>>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, Error> {
    if !authorized(token.as_deref(), request.headers()) {
        return Err(Error::Forbidden {
            message: "Private projection endpoint unavailable or unauthorized".into(),
        });
    }
    Ok(next.run(request).await)
}

fn authorized(expected: Option<&str>, headers: &HeaderMap) -> bool {
    let Some(expected) = expected.filter(|value| (32..=1024).contains(&value.len())) else {
        return false;
    };
    let Some(supplied) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    if supplied.len() > 1024 {
        return false;
    }
    // blake3::Hash equality compares its fixed-size digest in constant time.
    blake3::hash(expected.as_bytes()) == blake3::hash(supplied.as_bytes())
}

#[derive(OpenApi)]
#[openapi(
    paths(get_head, get_inventory, get_changes),
    components(schemas(Checkpoint, InventoryPage, ChangesPage))
)]
pub struct PostProjectionApiDoc;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn private_routes_reject_disabled_missing_and_incorrect_tokens() {
        let secret = "a-long-server-token-with-at-least-32-bytes";
        for (configured, supplied, status) in [
            (None, Some(secret), StatusCode::FORBIDDEN),
            (Some("short"), Some("short"), StatusCode::FORBIDDEN),
            (Some(secret), None, StatusCode::FORBIDDEN),
            (Some(secret), Some("wrong"), StatusCode::FORBIDDEN),
            (Some(secret), Some(secret), StatusCode::OK),
        ] {
            let token = configured.map(Arc::<str>::from);
            let app = Router::new()
                .route("/probe", get(|| async { "ok" }))
                .layer(middleware::from_fn_with_state(token, authorize));
            let mut request = Request::builder().uri("/probe");
            if let Some(supplied) = supplied {
                request = request.header("authorization", format!("Bearer {supplied}"));
            }
            let response = app
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status);
        }
    }

    #[test]
    fn cursor_reset_uses_gone_not_success_or_generic_server_failure() {
        use axum::response::IntoResponse;
        let response = map_error(ProjectionError::ResetRequired("expired")).into_response();
        assert_eq!(response.status(), StatusCode::GONE);
    }
}
