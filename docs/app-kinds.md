# App-kind posts

Nexus indexes public app posts at
`/pub/<namespace>/v1/posts/{id}/{editId}[-{slug}].json`. `social` uses the social
reader and `pubky.app` keeps its legacy route. Every other namespace is accepted
if it has 1–32 ASCII characters, starts with a lowercase letter, and contains only
lowercase letters, digits or `-`. Nexus has no namespace allowlist or per-app
configuration. Every accepted app post receives Resource metadata and a resource
feed reference, regardless of its namespace or kind.

Kind tokens have 1–32 ASCII characters, start with a lowercase letter, and contain
only lowercase letters, digits, `_` or `-`. For example, an app can use namespace
`example` and kind `note`; Nexus assigns no meaning to either token.

App payloads use `pubky-social-specs`' `PostEnvelope<AppKind>` from the pinned
`1.0.0-rc.1` crate. The shared reader validates fields, byte/attachment limits, references
and timestamp IDs. `AppKind` accepts lowercase tokens without content semantics.
`content` is a required string; its contents and unknown fields (including `ext`)
are never interpreted or stored by Nexus. IDs come from the path, not a required
body member. Invalid payloads fail as non-retryable `SpecValidation`. Private,
malformed and versionless storage events do not index posts. Slugs are decoration.

## One Resource per URI

An app post is the **Resource node of its normalized versionless URI**,
for example `pubky://<author>/pub/example/v1/posts/<id>`. Its `id` is computed with
nexus-common's `normalize_uri` and `resource_id`, just like any URI. Namespace and
author are part of the URI, so identical timestamp IDs in different namespaces or
authors cannot collide. There is no separate `AppPost` graph node.

An `AUTHORED` edge connects the author. The Resource carries `namespace`, `kind`,
`post_id` (the path timestamp ID), `author`, `path`, parent/embed references and
`version_uri` (the last accepted file, including its slug). `app_indexed_at` records
its first app acceptance; the Resource's original `indexed_at` survives
if a tag or reply created it first. Content, extensions, attachments and lock
payloads are not stored on it.

Social v1 tags on an app URI use the existing Resource tag path. Social replies
and app parent/embed references to it merge that same Resource, even before the
app object is indexed. Later app PUTs add metadata to that node. A reference to an
app post version in a parent/embed resolves to the logical app URI. Tags retain
their target URI as usual: explicitly tagging a version file names that file's
Resource, while tagging the logical post names the versionless Resource.

App parents/embeds use `APP_REPLIED` and `APP_EMBEDDED`; social post targets retain
their social `Post` nodes. Missing social-post or author dependencies retry without
partial writes. Other valid URI targets merge Resources without fetching them.
The envelope's existing reference validation and accepted schemes still apply.

App-origin edges do not contribute to social reply/repost counts or lists, and do
not generate notifications, mentions or search entries. A social reply to an app
URI has a `REPLIED` edge to its Resource, exposes the logical URI through
`relationships.replied`, and enters the author's replies stream. Its stored
`app_parent` marker preserves reply classification and parent immutability. It
does not enter root timelines or notify the app parent. No app reply-count or
reply-list endpoint is provided. Default social streams and social by-ID hydration
remain scoped to social `Post` nodes.

## Versions and deletion

Each accepted version PUT updates the logical app metadata. As in #1161/N3, the
last processed event wins, including replay of an older edit; Nexus does not
compare edit IDs to an indexed head. The shared reader requires valid timestamp
bounds and `editId >= id`.

**DEL of any version removes the logical app metadata from the index**, its
`AUTHORED` edge and outgoing app edges, following N3's logical-post deletion rule.
The underlying URI Resource and incoming tags/replies remain: their writers still
reference that URI. The post disappears from namespace streams. A later PUT can
restore its app metadata on the same node. Deleting an old file is therefore not
merely pruning Nexus's indexed history.

The resource feed reports the actual deleted version file. It does **not** claim
that other stored versions were deleted. Whole-post deletion requires the source's
individual version DELs; Nexus emits no synthetic versionless or null-path DEL.
Consumers can retain other source versions until their DELs arrive, independently
of Nexus's logical graph policy.

## Explicit app stream

```text
GET /v0/stream/posts?namespace=example&kind=note
GET /v0/stream/posts?namespace=example&exclude_kinds=memo,article
GET /v0/stream/posts?namespace=example&source=author&author_id=<pk>
```

Only an explicit app namespace selects app metadata from Resource nodes.
Absent, `pubky.app` or `social` selects the existing social stream. App selection
supports `source=all|author`, timeline sorting, ascending/descending order,
skip/limit, start/end bounds, author filtering and mutually exclusive `kind` or
`exclude_kinds` (up to 32 tokens). Equal timestamps sort by full URI. Graph-reach
sources, engagement sorting, viewer flags, tags, post IDs and attachment hydration
return 400. `/v0/stream/posts/keys` rejects app namespaces. Empty results are `200 []`.

The response's `id` is the canonical Resource hash; `post_id` is the timestamp ID.
`indexed_at` is the first app acceptance timestamp. `version_uri` is the source
file to fetch. No content, extensions, attachment or lock payload is returned.
The same node is also accessible through normal Resource by-ID/URI hydration and
its tag routes.

Social posts keep their social indexing and do not emit `kind:post` resource
references. App namespace handling has no social-kind selection configuration.

## Resource changes contract — fork extension

`GET /v0/resources/changes?cursor=0&limit=500` reads `Event:ResourceChanges`.
**`post` is a fork-only reference kind meaning “the object names itself”.** App
references use the existing resource line schema:

```json
{"op":"put","resource_id":"<hash of normalized URI>","uri":"pubky://<author>/pub/example/v1/posts/0032SSN7Q4EVG","kind":"post","author":"<author>","object_path":"/pub/example/v1/posts/0032SSN7Q4EVG/0032SSN7Q4EVJ.json","at":1792980000000}
```

`uri` is the normalized versionless URI; `object_path` is the exact version file,
including any slug. Fetch `pubky://{author}{object_path}`. `kind` is the reference
kind, not the namespace-owned content kind. The line contains no namespace,
post-kind token, content or extension fields. `at` is Unix milliseconds.

Delivery is **at least once, in append order**. Consumers apply PUT as an upsert
keyed by **`(author, object_path)`** and DEL as a delete by that same key. The URI
identifies the logical Resource; multiple stored versions can name it. Duplicates
after retries are expected and must be harmless. App DELs can repeat and can name
versions whose PUT was not observed by this indexer.

The graph write precedes append. A retry after graph commit repeats the upsert
and appends; a retry after append can duplicate the line. Pending deletes are
saved before graph removal. Append and pending-record cleanup are atomic. The
same transaction maintains content-free per-version metadata in
`Event:ResourceChanges:PostVersions`, covering all app versions. Version metadata
survives removal of the logical graph metadata until each file's DEL. PUT
supersedes an unfinished DEL of that version. No cross-database transaction is
claimed.

Cursor, limit, epoch and seed rules are the resource feed's existing rules: an
atomic page returns `lines`, `next_cursor`, `x-events-length` and optional
`x-events-epoch`. Reads never initialize state. Persist epoch and cursor together;
on epoch change or cursor beyond that response's length, rebuild from cursor 0.
Retain the prior epoch when absent. Limits are 1–1000 (default 500).

`resource-changes-seed` snapshots Resource tags, post version metadata and graph
fallback for app heads. It then copies the live append suffix and atomically
publishes a new epoch under the existing lease.
Snapshot/suffix duplicates are harmless; interrupted seeds retain the old feed.
Redis persistence is required for historical versions and pending deletes.
After Redis loss, graph fallback recovers current heads; other versions require
source replay. The feed records accepted storage events, not unrelated graph
maintenance or profile cascades.

Reseeding ignores obsolete social post-version metadata and social graph feed
markers from earlier deployments. Existing feed history stays append-only until
an explicit reseed publishes a new epoch.
