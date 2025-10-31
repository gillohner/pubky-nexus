use crate::routes::v0::endpoints::ALARM_ROUTE;
use crate::{Error, Result};
use axum::extract::Path;
use axum::Json;
use nexus_common::models::alarm::AlarmDetails;
use tracing::info;
use utoipa::OpenApi;

#[utoipa::path(
    get,
    path = ALARM_ROUTE,
    description = "Alarm view",
    tag = "Alarm",
    params(
        ("author_id" = String, Path, description = "Author Pubky ID"),
        ("alarm_id" = String, Path, description = "Alarm Crockford32 ID")
    ),
    responses(
        (status = 200, description = "Alarm", body = AlarmDetails),
        (status = 404, description = "Alarm not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn alarm_view_handler(
    Path((author_id, alarm_id)): Path<(String, String)>,
) -> Result<Json<AlarmDetails>> {
    info!(
        "GET {ALARM_ROUTE} author_id:{}, alarm_id:{}",
        author_id, alarm_id
    );

    match AlarmDetails::get_by_id(&author_id, &alarm_id).await {
        Ok(Some(alarm)) => Ok(Json(alarm)),
        Ok(None) => Err(Error::AlarmNotFound {
            author_id,
            alarm_id,
        }),
        Err(source) => Err(Error::InternalServerError { source }),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(alarm_view_handler),
    components(schemas(AlarmDetails))
)]
pub struct AlarmViewApiDoc;
