# Custom post kinds and universal embeds (PoC)

Apps can publish their own kinds of social posts without teaching Nexus each
kind's schema. An event or map entry is still a Post: it has an author, can be
tagged or replied to, and appears in the existing social feeds. Only the app
interprets its custom content.

This is a v0-path experiment to inform [the draft v1 social specs](https://github.com/pubky/pubky-app-specs/pull/142).
It is not a v1 implementation or a stable contract.

## Writing a post

Write JSON to `/pub/pubky.app/posts/{post_id}` on the author's homeserver:

```json
{
  "kind": "event",
  "content": "{\"title\":\"Picnic\",\"lat\":52.52,\"lon\":13.405}",
  "embed": "geo:52.52,13.405"
}
```

- `kind` is an exact, case-sensitive string: `event` and `Event` are different.
  It must be 1–128 UTF-8 bytes, without whitespace, control characters or commas.
  The literal `unknown` is accepted and filterable; it is not a catch-all.
- `content` remains a **string**, not an arbitrary JSON value. Custom content is
  preserved verbatim, including whitespace. A custom post's entire JSON document
  is limited to 512 KiB. It must still have content, attachments or an embed, and
  cannot use the reserved exact content `[DELETED]`.
- Built-in `short`, `long`, `image`, `video`, `link`, `file` and `collection`
  retain their specs content validation and sanitization. Custom content is not
  parsed for mentions. Full-text search treats it as text, not structured data.
- `parent`, `attachments`, `lock` and the post ID keep their v0 rules. A parent
  must be a Pubky Post, never an external URL. Collections still reject embeds.
- New embed input is a URI string. Legacy `{ "uri": "…", "kind": "link" }`
  objects remain readable under their old validation and URL cleanup rules.
  Output always uses a string or `null`; it never includes an embed kind.
- New string embeds are limited to 1,024 characters. The draft's frozen whitespace
  set is trimmed at the ends and rejected within the value, as are ASCII controls.
  Schemes are lowercased; the remainder is preserved. HTTP(S) uses the draft's
  lightweight prefix gate; opaque schemes such as `geo:`, `nostr:` and `ipfs:`
  are identifiers, not fetched locations. Pubky embeds require a valid key and
  a public `/pub/` path using the current parser; other `pubky*` schemes are reserved.

Nexus does not fetch or render external embeds. Acceptance is **not** a statement
that a URI is safe to open, executable, or even reachable. Clients must choose
which schemes to render or navigate to; never turn opaque content into trusted HTML.

## Existing endpoints, extended behavior

No routes are added or renamed. Post views retain their existing envelopes.

| Endpoint | Change |
| --- | --- |
| `GET /v0/post/{author_id}/{post_id}/details` | Returns the exact `kind`, `content`, and new `embed: string \| null`. |
| `GET /v0/post/{author_id}/{post_id}` | Same fields under `details`; existing counts, tags and relationships remain. |
| `GET /v0/stream/posts` | Includes custom kinds by default. `kind=event` selects exactly that kind; `exclude_kinds=event,collection` excludes those kinds before pagination. |
| `GET /v0/stream/posts/keys` | Same filtering behavior, returning the existing keys/cursor response. |
| `POST /v0/stream/posts/by_ids` | Hydrates custom posts with the same details shape as other post views. |
| `GET /v0/search/posts/by_content?q=picnic&kind=event` | Searches custom content as text, with an exact optional kind filter. Optional author scope remains `author=…`. |
| `GET /v0/resource/by-uri?uri=…` and `GET /v0/resource/{resource_id}/tags` | Also find embed-only Resources, with an empty `tags` array when nobody has tagged them. URL-encode the lookup URI. |
| `GET /v0/stream/resources` and `/v0/stream/resources/ids` | Also include embed-only Resources, including zero-score entries under `sorting=taggers_count`. |

`kind` and `exclude_kinds` remain mutually exclusive. Exclusion accepts 1–7 CSV
tokens, deduplicates them, and trims whitespace around tokens. Collection and
reply sources continue to reject both parameters. No filter means all kinds,
not just the built-in vocabulary. Existing notification kind fields also preserve
custom strings.

## Graph and cache behavior

A custom post is a normal `Post` node, not a generic Resource. A Pubky Post embed
uses the existing `REPOSTED` relationship. Other embeds use
`(post)-[:EMBEDS {app: 'pubky.app'}]->(resource:Resource)`.

Resource identity uses the **existing universal-tag normalization and hash**.
For example, a tag and embed differing only by a web fragment share a node.
`details.embed` keeps the submitted string (after the input cleanup above);
`Resource.uri` is the normalized identity. Non-post Pubky references are Resource
targets too; profile embeds do not create User tag relationships.

Changing or removing an embed removes its old relationship. A Resource is deleted
only after its last relationship is gone: removing a tag must not delete a node
still referenced by an embed, and vice versa. Replays preserve Resource creation
time. Switching between external embeds and reposts refreshes relationships and
the affected posts' repost counts. Custom-kind edits clear old mention edges.

Unfiltered Resource streams use Neo4j because the Redis Resource indexes only
track tags. `app=pubky.app` also matches EMBEDS references; tag-filtered queries
still require matching tags. Both paths honor inclusive `start` (upper) / `end`
(lower) score bounds and `skip` / `limit`. Scores alone cannot page through ties;
use offsets within equal-score results. This graph fallback is a PoC performance
tradeoff, not a new Redis indexing scheme.

## Compatibility and rollout

No automatic data rewrite is included. Old graph nodes and cached JSON without
`embed` still read as `null`; existing kind values stay readable. Schema fields
and relationships are additive, and the existing Post kind/search indexes work
with arbitrary strings.

Previously rejected custom posts and previously discarded external embeds are
not recoverable from Redis or the derived graph alone. Test on a fresh PoC index,
or explicitly reprocess the affected current public homeserver records through
the watcher. Rebuilding only Redis is insufficient. Existing consumers should
handle unfamiliar kind strings before pointing at this fork. Older Nexus binaries
may again collapse or reject them on rollback; preserve the source records.

Still out of scope: v1 paths/epochs, post versioning, the `short` kind rename,
structured attachment changes, unknown-field round-tripping, arbitrary object
namespaces, private indexing, external fetching, and reverse embed lookup.
The string embed shape and opaque custom content follow the draft direction;
v0 Resource normalization/IDs and existing Pubky parsing deliberately remain.

## Focused verification

With the standard local Neo4j and Redis Stack test services running:

```sh
cargo test -p nexus-watcher --test custom_posts
cargo test -p nexus-webapi --test custom_posts
cargo test -p nexus-common --lib models::post
cargo test -p nexus-webapi --lib
cargo clippy --workspace --all-targets -- -D warnings
```

The two `custom_posts` binaries create their own fixtures and do not need a
homeserver or Postgres. They exercise cache misses, case-sensitive and punctuation
kinds, search escaping, HTTP responses, shared tag/embed lifecycle, repost count
transitions, and ordinary post deletion.
