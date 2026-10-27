use crate::models::{BoundedPagination, PostId, PubkyId, Tags};
use crate::routes::{AppState, Path, Query};
use crate::{Error, Result};
use axum::{routing::get, Json, Router};
use nexus_common::models::social_event::{self, Attendee, EventQuery, EventView, Occurrence};
use serde::Deserialize;
use utoipa::{OpenApi, ToSchema};

#[derive(Default, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    #[default]
    All,
    Following,
}
#[derive(Default, Deserialize)]
pub struct StreamQuery {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub tags: Option<Tags>,
    pub collection: Option<String>,
    pub author: Option<PubkyId>,
    #[serde(default)]
    pub source: EventSource,
    pub user_id: Option<PubkyId>,
    pub sort: Option<String>,
    #[serde(flatten)]
    pub pagination: BoundedPagination<10_000, 20, 200>,
}
#[derive(Default, Deserialize)]
pub struct AttendanceQuery {
    pub recurrence_id: Option<String>,
    pub status: Option<String>,
    #[serde(flatten)]
    pub pagination: BoundedPagination<10_000, 20, 200>,
}
fn check_recurrence(value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        social_event::calendar::local(value).map_err(|_| {
            Error::invalid_input("recurrence_id must be a local YYYY-MM-DDTHH:MM:SS")
        })?;
    }
    Ok(())
}

#[utoipa::path(get,path="/v0/stream/events",tag="Events",
    description="Indexed social event occurrences in start order. Times are UTC milliseconds. Returns occurrences overlapping [from, to): end_at > from AND start_at < to. Defaults to now through twelve months ahead, including events already in progress. Recurrences are materialized on writes and rolled daily; requests never expand rules. One row per master/recurrence_id, with post_uri naming its current override. No location or bounding-box filtering.",
    params(
      ("from"=Option<i64>,Query),("to"=Option<i64>,Query),
      ("tags"=Option<Tags>,Query,description="All supplied tags must match the master or override"),
      ("collection"=Option<String>,Query,description="Versionless public collection URI"),
      ("author"=Option<PubkyId>,Query),("source"=Option<EventSource>,Query),
      ("user_id"=Option<PubkyId>,Query,description="Required for source=following"),
      ("sort"=Option<String>,Query,description="start (ascending)"),
      ("skip"=Option<usize>,Query),("limit"=Option<usize>,Query,description="1–200, default 20")
    ),responses((status=200,body=Vec<Occurrence>),(status=400,description="Invalid query"),(status=500,description="Internal error")))]
pub async fn events(Query(q): Query<StreamQuery>) -> Result<Json<Vec<Occurrence>>> {
    let now = chrono::Utc::now();
    let from = q.from.unwrap_or(now.timestamp_millis());
    let to =
        q.to.unwrap_or_else(|| social_event::calendar::window_end(now));
    if from < 0 || to <= from || to.saturating_sub(from) > 366 * 86_400_000 {
        return Err(Error::invalid_input(
            "from/to must be nonnegative UTC milliseconds with from < to and a span of at most 366 days",
        ));
    }
    if q.sort.as_deref().is_some_and(|s| s != "start") {
        return Err(Error::invalid_input("Events sort must be start"));
    }
    let following = match q.source {
        EventSource::All => None,
        EventSource::Following => Some(
            q.user_id
                .ok_or_else(|| Error::invalid_input("source=following requires user_id"))?
                .to_string(),
        ),
    };
    if let Some(uri) = &q.collection {
        let parsed = pubky_social_specs::ParsedUri::try_from(uri.as_str()).map_err(|_| {
            Error::invalid_input("collection must be a public versionless post URI")
        })?;
        if parsed.visibility != pubky_social_specs::Visibility::Public
            || !matches!(
                parsed.resource,
                pubky_social_specs::Resource::Post { version: None, .. }
            )
        {
            return Err(Error::invalid_input(
                "collection must be a public versionless post URI",
            ));
        }
    }
    Ok(Json(
        social_event::stream(EventQuery {
            from,
            to,
            tags: q.tags.map(|t| t.to_string_vec()).unwrap_or_default(),
            collection: q.collection,
            author: q.author.map(|u| u.to_string()),
            following,
            skip: q.pagination.skip_value(),
            limit: q.pagination.limit_value(),
        })
        .await?,
    ))
}

#[utoipa::path(get,path="/v0/event/{author}/{id}",tag="Events",
    description="A native social event with attendance counts and up to 50 next indexed occurrences. recurrence_id selects an occurrence's effective RSVP counts; its explicit RSVP overrides the author's series RSVP. Going includes accepted attendees and guests, except ticket policy ignores guests. Requested is never counted as going.",
    params(("author"=PubkyId,Path),("id"=PostId,Path),("recurrence_id"=Option<String>,Query)),
    responses((status=200,body=EventView),(status=400,description="Invalid query"),(status=404,description="Event not found"),(status=500,description="Internal error")))]
pub async fn event(
    Path((author, id)): Path<(PubkyId, PostId)>,
    Query(q): Query<AttendanceQuery>,
) -> Result<Json<EventView>> {
    check_recurrence(q.recurrence_id.as_deref())?;
    let view = social_event::event_view(&author, &id, q.recurrence_id.as_deref())
        .await?
        .ok_or_else(|| Error::post_not_found(author, id))?;
    Ok(Json(view))
}

#[utoipa::path(get,path="/v0/event/{author}/{id}/attendees",tag="Events",
    description="One effective RSVP per author, ordered by original indexed_at then author (FIFO waitlist order). Newest indexed content wins across duplicate RSVP posts; retries do not move the attendee. An occurrence-specific answer supersedes the series answer. Guests count as seats except with ticket policy. waitlisted is informational; Nexus grants no admission rights.",
    params(("author"=PubkyId,Path),("id"=PostId,Path),("recurrence_id"=Option<String>,Query),("status"=Option<String>,Query,description="requested, accepted, declined or tentative"),("skip"=Option<usize>,Query),("limit"=Option<usize>,Query)),
    responses((status=200,body=Vec<Attendee>),(status=400,description="Invalid query"),(status=404,description="Event not found"),(status=500,description="Internal error")))]
pub async fn attendees(
    Path((author, id)): Path<(PubkyId, PostId)>,
    Query(q): Query<AttendanceQuery>,
) -> Result<Json<Vec<Attendee>>> {
    check_recurrence(q.recurrence_id.as_deref())?;
    if q.status
        .as_deref()
        .is_some_and(|s| !matches!(s, "requested" | "accepted" | "declined" | "tentative"))
    {
        return Err(Error::invalid_input("Invalid RSVP status"));
    }
    let uri = pubky_social_specs::post_uri_builder(author.to_string(), id.to_string());
    if social_event::validate_parent(&uri).await? != Some(true) {
        return Err(Error::post_not_found(author, id));
    }
    Ok(Json(
        social_event::attendees(
            &uri,
            q.recurrence_id.as_deref(),
            q.status.as_deref(),
            q.pagination.skip_value(),
            q.pagination.limit_value(),
        )
        .await?,
    ))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v0/stream/events", get(events))
        .route("/v0/event/{author}/{id}", get(event))
        .route("/v0/event/{author}/{id}/attendees", get(attendees))
}
#[derive(OpenApi)]
#[openapi(
    paths(events, event, attendees),
    components(schemas(EventSource, Occurrence, EventView, Attendee))
)]
pub struct SocialEventsApiDoc;
