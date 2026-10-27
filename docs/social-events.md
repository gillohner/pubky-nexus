# Native social events, RSVPs and ratings

Nexus indexes public social `event`, `rsvp` and `rating` posts itself. Files use
`/pub/social/v1/posts/{id}/{editId}.json`; logical identity is the versionless URI.
The social 1.1 draft validator is pinned to
[`8e7f1adeba47a33b9e31382a3462a1583bc291ba`](https://github.com/pubky/pubky-social-specs/commit/8e7f1adeba47a33b9e31382a3462a1583bc291ba)
until a release is available. Existing v0 and v1 post spellings remain supported.
Private documents are not indexed. These projections add no resource-feed lines.
Ordinary universal reply/embed references retain their existing feed behavior.

## Calendar projection

Events store `start_at` and `end_at` as UTC milliseconds, plus `all_day`, `status`,
`policy`, `capacity` and `waitlist`. Location URIs use the common Resource
normalizer and ID function; each is connected by `LOCATED_AT`. Nexus performs no
geocoding or spatial filtering. Event title and description are searchable via
`GET /v0/search/posts/by_content?q=...&kind=event`.

Time conversion uses the IANA database bundled with chrono-tz. Following
[RFC 8984 sections 1.4.5–1.4.6](https://www.rfc-editor.org/rfc/rfc8984.html#section-1.4.5),
clock gaps and folds use the offset before the transition. Duration days/weeks
advance the local date; hours/minutes/seconds advance elapsed time. Consequently,
`P1D` and `PT24H` can differ across daylight-saving transitions. An all-day event
without duration defaults to one local calendar day.

At write time, Nexus expands the draft's daily, weekly, monthly and yearly rules,
including interval, week start, month/day filters, ordinal weekdays, set positions,
count/until and excluded dates. Ordinal weekdays require monthly/yearly frequency.
The lifetime is bounded to 1,000 occurrences and three calendar years from DTSTART;
a start more than three years in the future is rejected. Invalid month days are
omitted; exclusions apply after count. Only ongoing/upcoming instances whose start
is before twelve months ahead are materialized. Past occurrences are not an archive.

Each occurrence is keyed by `(master URI, recurrence_id)`, where `recurrence_id` is
the original local start. `post_uri` identifies the master or overriding post.
Same-author overrides replace the whole instance; the newest original `indexed_at`
wins (post ID breaks ties). Deleting an override restores the next surviving override
or generated instance. An override whose key no longer exists becomes an ordinary
event under its own URI. Source snapshots and a graph lock prevent a racing rebuild
from replacing newer input with stale results.

The `event-occurrences` job rolls the window daily at 04:20 UTC. It also drains
pending change notifications. `orphan-rsvps` runs Sundays at 04:40 UTC, removing the
attendance projection for missing recurrence keys while preserving the source reply.
The complete bounded recurrence set is used for cleanup, so a valid instance outside
the twelve-month window is not an orphan. Run either job manually with
`nexusd jobs run <name>`; configure schedules under `[jobs.<name>]`.
Replies to an override resolve its master series before checking recurrence keys.

## Attendance and ratings

An RSVP must target an indexed, undeleted event. A missing parent is retried as a
dependency; an indexed non-event parent is rejected. One effective RSVP is selected
per author and event/occurrence. The newest indexed content wins across duplicate
RSVP posts; an unchanged retry does not change that order. An occurrence-specific
answer, including a decline, supersedes a series answer. A reply directly to the
active override also applies to that occurrence.

Counts separate requested, accepted, declined and tentative. `going` is the accepted
headcount plus guests; ticket policy ignores guests. `requested` never counts as
`going`. Attendees are ordered by their winning post's original `indexed_at`, then
author. When waitlisting is enabled, accepted parties exceeding cumulative capacity
are marked `waitlisted`; this count is included in `going`, not subtracted from it.
This is public attendance information, not an admission or ticket entitlement check.

Ratings accept integers **1–10** and require a parent URI. The newest indexed rating
per author/parent wins; edits and deletes recompute the target's `rating_avg` and
`rating_count`, with zero/zero for no ratings. Both Post and Resource views expose
these fields. A missing social or legacy pubky post target is retried as a dependency;
its rating never falls back to a Resource node. Other URI targets use Resource nodes.
Aggregate updates and view hydration seek posts by indexed URI and resources by ID.
Existing post/resource streams support `sorting=rating`; score cursors
can be fractional. Other sorting modes keep integer timestamp/count JSON values.

RSVPs and ratings retain ordinary reply notifications. `event_changed` is sent to
accepted attendees when an indexed occurrence's start, normalized location set or
status changes, or a future occurrence is removed by an edit/delete. The notification
contains `event_uri`, `recurrence_id` and `post_uri`. Occurrence-specific declines
suppress a series acceptance. Notification records are persisted with the occurrence
write and retried idempotently after a Redis failure; a graph commit followed by a
failed notification append does not lose the notification. Natural window aging does
not emit change notifications. Events beyond the materialized window have no
occurrence-change notifications until they enter it.

## API

- `GET /v0/stream/events?from=...&to=...`: UTC milliseconds, returning occurrences
  overlapping `[from, to)` (`end_at > from AND start_at < to`), at most 366 days per
  query; defaults now through twelve months ahead, including events already in progress. Optional
  `tags` (all must match master or override), versionless `collection`, `author`,
  `source=all|following` and `user_id` (required for following). `sort=start` is the
  only order, ascending, with master/key tie-breakers. `skip` is at most 10,000;
  `limit` is 1–200, default 20. Empty results return `200 []`.
- `GET /v0/event/{author}/{id}`: post view, event properties, counts and up to 50 next
  occurrences. Optional `recurrence_id` selects effective attendance counts; event
  properties still describe the requested post, and occurrences identify overrides.
- `GET /v0/event/{author}/{id}/attendees`: effective RSVP posts, optional
  `recurrence_id` and `status=requested|accepted|declined|tentative`, same pagination.
  Missing/non-event IDs return 404; malformed queries return 400.

Requests read precomputed occurrences and never expand rules. Attendance currently
selects all effective RSVPs for an event before applying pagination, to compute FIFO
capacity consistently. Rating fields are read from graph aggregates so cached post
bodies cannot hide an updated score. Historic posts previously indexed as unknown
need their source versions replayed through the watcher to gain native projections.
