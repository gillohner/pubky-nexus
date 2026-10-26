# App-kind posts

Nexus indexes public app posts at
`/pub/<namespace>/v1/posts/{id}/{editId}[-{slug}].json`. `social` uses the social
reader and `pubky.app` keeps its legacy route. App namespaces are 1–64 ASCII bytes,
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
logical app post and all its edges**; subsequent version DELs are no-ops. Deleting
an old version is not history pruning. A later PUT can recreate the post.

## Durable and feed-only tiers

Every valid app PUT emits a reference-feed line. Durable kinds also write graph
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
node and edges; moving to durable creates them. Redis retains only the latest
feed-only line under `Event:ResourceChanges:FeedOnlyAppPosts`, keyed by logical URI,
to support deletes and reseeding. No app content or extensions are cached there.

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
GET /v0/stream/posts?namespace=music&exclude_kinds=review,session
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

## Reference feed

The existing `GET /v0/resources/changes` cursor, epoch and ordering apply to both
Resource-tag and app-post lines. App lines contain exactly these five fields:

```json
{"op":"put","author":"<pk>","path":"/pub/music/v1/posts/<id>","namespace":"music","kind":"listen"}
```

`path` is the **versionless logical path**, so all edits share the consumer key
`(author, path)`. A consumer upserts on `put` and deletes that key on `del`. Delivery
is **at least once, in append order**; duplicate puts after retries are expected
and must be harmless. Resource-tag lines retain their `(author, object_path)`
key. Discriminate the shapes by `namespace`/`path`, not by `kind`, whose app
vocabulary is open. Feed-only consumers list the version directory to hydrate a
logical reference; the five-field line does not carry an edit ID.

The graph write precedes append for durable posts. A crash after graph commit is
recovered by retrying the upsert and appending the line; a retry after append may
append the same put again. Pending delete metadata survives removal of the graph
node, and delete publication/metadata removal is atomic. A DEL for an object never
accepted here has no known kind and emits no line; repeated completed DELs are
no-ops. A later put supersedes an unfinished delete for the same logical path.

Reseeding snapshots Resource-tag edges and durable app nodes, then current
feed-only metadata, and atomically appends the old list's live suffix before
publishing the new epoch. Consumers rebuild their state on an epoch change;
snapshot/suffix duplicates are harmless upserts. Redis persistence is needed for
feed-only metadata and pending deletes. After Redis loss, the graph can restore
durable state but feed-only state requires replay/re-ingestion of source events.
The feed records accepted events, not unrelated graph maintenance or profile
cascades. See [Resource reference feed](resource-changes.md) for cursor handling.
