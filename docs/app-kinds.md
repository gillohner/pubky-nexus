# App-kind posts

Nexus indexes public app posts at
`/pub/<namespace>/v1/posts/{id}/{editId}[-{slug}].json`. `social` uses the social
reader and `pubky.app` keeps its legacy route. App examples use `music`
(`listen`, `listening`, `session`), `maps` (`route`), `events` (`availability`) and
`marketplace`. The extension hook also accepts unconfigured namespaces. App namespaces are 1–64 ASCII bytes,
start with a lowercase letter, and otherwise contain lowercase letters, digits,
`.`, `_` or `-`. Kind tokens are 1–32 ASCII bytes, start with a lowercase letter,
and otherwise contain lowercase letters, digits, `_` or `-`.

App payloads use `pubky-social-specs`' `PostEnvelope<AppKind>` at the pinned v1
revision. The shared reader validates the envelope, byte and attachment limits,
references, and timestamp IDs. `AppKind` accepts lowercase tokens and imposes no
content semantics. `content` is a required string; its contents and unknown fields
(including `ext`) are never interpreted or stored by Nexus. IDs come from the path;
the v1 envelope has no required `id` member. Invalid payloads are `SpecValidation`
errors and are not retried. Private, malformed and versionless storage events do
not index posts. Slugs are decoration, not identity.

## Identity and versions

A durable post has a separate `AppPost` label, uniquely keyed by its full
**versionless URI**, for example
`pubky://<author>/pub/music/v1/posts/<id>`. The same ID in music, maps or a social
post therefore cannot collide. An `AUTHORED` edge connects the author. Stored
metadata includes namespace, kind, parent/embed, the original indexed timestamp,
and `version_uri`, the latest accepted file's address for hydration.

As for social v1 posts, each accepted version PUT updates the same logical node.
The last processed event wins, even if an older edit is replayed after a newer one;
Nexus does not compare edit IDs to an indexed head. The shared reader requires
`editId >= id` and valid timestamp bounds. **DEL of any version deletes the entire
logical app post and all its edges**; subsequent version DELs are graph no-ops, but each accepted stored version still
gets its own feed DEL. Deleting
an old version is not history pruning. A later PUT can recreate the post.

## Durable and feed-only tiers

Every valid app PUT emits an app-post feed line. Durable kinds also write graph
metadata. Feed-only kinds write no graph node or Resource edges. `feed_only` is an
indexing policy: the homeserver file remains available and Pinakes retains the data
it indexes. Nexus does not impose an expiry or delete the source file.

```toml
[watcher.app_kinds.music]
feed_only = ["listening"]
```

Use `[app_kinds.music]` in a standalone watcher config. `listen` is durable public
listen history; only `listening` defaults to feed-only. An unconfigured namespace
has no feed-only kinds. An explicit `app_kinds` map replaces the default map; use
an empty `feed_only` array to make every kind in a configured namespace durable.
Operator configuration copies the app spec's tier list. Config changes take effect
on subsequent events; they do not scan or reclassify existing posts.

A kind edit can move between tiers. Moving to feed-only removes a prior durable
node and edges; moving to durable creates them. Redis retains metadata for every accepted stored version under
`Event:AppPostChanges:Versions` until that version is deleted. This covers both
tiers and supports accurate version DELs and reseeding after the graph node is
removed. No app content or extensions are cached there.

## Graph relationships and visibility

Durable app parents/embeds link to social posts, durable app posts or normalized
Resources for other valid URI targets. Missing post/author dependencies retry
without leaving a partial node. App references to stored versions resolve to the
logical app URI. The shared envelope's reference rules apply; Nexus does not
expand its accepted schemes or fetch external targets. A feed-only post cannot
satisfy a graph post dependency while it has no durable node.

App-origin edges use `APP_REPLIED` and `APP_EMBEDDED`. App posts do not contribute
to social reply/repost counts or reply lists, and do not generate notifications,
mentions or search entries. Default social streams and social by-ID hydration
remain scoped to `Post` nodes.

A social post may reply to a durable app post. It has a `REPLIED` edge to the
`AppPost`, exposes its logical URI through `relationships.replied`, and appears in
the social author's replies stream. It does not notify the app parent or enter
root timelines/engagement streams. A stored `app_parent` marker preserves that
classification and parent URI if the app node is later removed. Social parent
immutability still applies. No new app reply-count/list endpoint is provided.

## Explicit app stream

```text
GET /v0/stream/posts?namespace=music&kind=listen
GET /v0/stream/posts?namespace=music&exclude_kinds=listening,session
GET /v0/stream/posts?namespace=music&source=author&author_id=<pk>
```

Only an explicit app namespace returns durable `AppPost` metadata. Absent,
`pubky.app` or `social` selects the existing social stream. App selection supports
`source=all|author`, timeline sorting, ascending/descending order, skip/limit,
start/end timestamp bounds, an author filter, and mutually exclusive `kind` or
`exclude_kinds` (up to 32 tokens). Equal timestamps are ordered by full URI.
Graph-reach sources, engagement sorting, viewer flags, tags, post IDs, and
attachment hydration are rejected with 400. `/v0/stream/posts/keys` cannot encode
app identity and rejects app namespaces. Empty app results return `200 []`.

The metadata's `version_uri` is the source file to fetch. The response contains no
content, extensions, attachments or lock payload. Feed-only posts have no stream
entry.

## Selected social posts

```toml
[watcher.post_feed]
social_kinds = ["event", "rsvp"]
```

Use `[post_feed]` in the standalone watcher config. These defaults select public
`/pub/social/v1/posts/{id}/{editId}.json` posts for the app-post feed too. An
explicit list replaces the defaults; `[]` disables social feed references.
Selection affects subsequent events and does not reclassify old versions.

The watcher reads the kind with `PostEnvelope<AppKind>` at the unchanged
`pubky-social-specs` v1 pin `acde362`. Known social kinds retain the pinned
content validation; other valid lowercase tokens use the catch-all envelope and
N3's existing `Unknown` storage kind. Social posts still use `Post` nodes, social
streams, relationships and indexing, independently of feed selection. They never
become `AppPost` nodes. The line preserves the original token as `post_kind` and
uses `namespace: "social"`; Pinakes fetches time, venue and other content from the
stored version. Nexus does not send those fields in the feed. No v1.1 dependency
or app-specific payload interpretation is added.

## App-post changes feed

`GET /v0/app-posts/changes?cursor=0&limit=500` reads the independent Redis list
`Event:AppPostChanges`. App posts never enter `Event:ResourceChanges` or
`GET /v0/resources/changes`; the resource feed retains the #1170 contract.

```json
{"op":"put","author":"<pk>","post":"/pub/music/v1/posts/0032SSN7Q4EVG","version":"/pub/music/v1/posts/0032SSN7Q4EVG/0032SSN7Q4EVJ.json","namespace":"music","post_kind":"listen","at":1792980000000}
```

`post` is the versionless identity path; the logical replay key is `(author, post)`.
`version` is the exact stored file path, including a slug when present. Fetch
`pubky://{author}{version}`; no directory listing is needed. `post_kind` is the
namespace's vocabulary. `at` is the acceptance/deletion time in Unix milliseconds.
No content, extension or temporal/venue payload is sent.

Delivery is **at least once, in append order**. PUT upserts the referenced version
within the logical post's version set; duplicate puts after retries must be
harmless. On DEL with a non-null `version`, remove only that stored version.
**`version: null` distinguishes a whole-post DEL**, which removes every version of
the logical key. PUT always has a non-null version.

The #1161/N3 graph policy remains: DEL of any stored version removes the logical
node. The feed reports the actual storage event, so it does **not** infer that the
other version files were deleted. N3 receives individual file DELs and does not
synthesize whole-post DELs. A consumer maintaining source history can retain the
remaining versions until their DELs arrive, independently of the graph policy.
Repeated completed DELs are no-ops. DEL of a version never accepted here emits no
line because its kind is unknown. Pending delete details are saved before graph
removal; append, removal of the pending record and version metadata removal are
atomic. A later PUT supersedes an unfinished DEL for that version.

The graph write precedes append for durable posts. A crash after graph commit is
recovered by retrying the upsert and appending; retry after append may duplicate
it. No cross-database transaction is claimed.

The response is `{ "lines": [...], "next_cursor": n }`. Cursor is a zero-based list
offset; limit is 1–1000, default 500. Invalid bounds return 400; an empty or exhausted
feed returns 200 with an empty array of lines and the requested cursor. Reads do
not initialize keys or fetch resources. `x-events-length` and optional
`x-events-epoch` come from the same atomic snapshot as the page. Persist cursor and
epoch together. On epoch change or cursor beyond that response's length, rebuild
local state from cursor 0. Retain the previous epoch when the header is absent.
Each feed has its own cursor and epoch; never reuse one feed's cursor on the other.

The `app-post-changes-seed` daemon job snapshots current version metadata and
recovers durable app and selected social heads from the graph if their append did not complete. It
then atomically appends the old list's live suffix and publishes a new epoch. A
lease protects publication; interrupted seeds leave the old generation intact.
Snapshot duplicates and concurrent suffix duplicates are harmless. Run the job
after upgrading all writers, and whenever a snapshot rebuild is needed. It does
not reseed or mutate the resource feed.

Redis persistence is required for prior-version, feed-only and pending-delete
metadata. After Redis loss, the graph can restore durable app and selected social heads, but historical
versions and feed-only objects require source replay/re-ingestion. The feed
records accepted events, not unrelated graph maintenance or profile cascades.
For an upgrade from a mixed-feed build, check for app lines in the old resource
list, re-ingest app source versions, and run the two seed jobs separately. Do not
copy the old five-field app lines: they lack a fetchable version. A resource
reseed changes its epoch so strict consumers rebuild using resource-only lines.
