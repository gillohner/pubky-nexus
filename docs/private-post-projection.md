# Private post replication and generic mentions

This branch extends the universal post contract in [custom-post-kinds.md](custom-post-kinds.md). It adds an authenticated source replication API and an optional mention-bearing text extension. Nexus does not parse event or calendar schemas, calculate occurrences, or introduce application-specific graph labels.

## Source and transaction contract

Every successful indexed Post create, update and hard deletion records an immutable source version in the same Neo4j transaction as the source mutation. The record contains the exact indexed `kind` and `content` strings plus `parent`, `embed`, `attachments` and `lock`. A deletion has `post: null`. The record describes committed graph source state; it does not wait for Redis, notifications or other derived indexes to finish.

All source mutations take one checkpoint write lock before acquiring Post locks. Inventory and change reads acquire the same checkpoint lock, so each bounded page and its checkpoint are consistent. This serializes source transactions and needs throughput testing before a busy deployment. A failed transaction cannot expose a source revision without its payload. Revision gaps are valid: an attempted write with a missing dependency may advance the checkpoint without creating a post.

Moderation paths using the shared hard-delete query produce deletion records. User deletion refuses to remove a user that still authors posts; posts must pass through the shared deletion path first. Direct administrative Cypher or older writers bypass this contract and require an explicit reset before replication resumes.

## Private HTTP contract

Set `NEXUS_PROJECTION_TOKEN` in the Nexus API process to a random server secret of 32–1,024 bytes. All three routes require `Authorization: Bearer <token>`. A missing or invalid configuration disables access with HTTP 403. Token comparison uses constant-time digest equality. Keep the token in server configuration and send it over a trusted private connection or HTTPS; never include it in browser configuration or public responses.

Historical source payloads may contain content that has since been removed or moderated. These routes belong behind the private server boundary. The token does not make replayed historical records suitable for public display.

| Route | Query | Result |
| --- | --- | --- |
| `GET /v0/projection/posts/head` | None | `epoch`, `revision`, `minimum_revision` |
| `GET /v0/projection/posts/inventory` | Required `epoch`, `since`; optional `after`, `kinds`, `limit` | Checkpoint fields, `items`, `next_cursor` |
| `GET /v0/projection/posts/changes` | Required `epoch`, `after`; optional `through`, `limit` | Checkpoint fields, `items`, `cursor`, `through`, `caught_up` |

All revision values are nonnegative decimal **strings**, including query parameters. Use arbitrary-precision integers when comparing them; JavaScript `Number` cannot represent all supported revisions exactly. `epoch` is an opaque instance generation. `minimum_revision` is the oldest valid replay cursor, inclusive.

Each item has this shape:

```json
{
  "revision": "42",
  "uri": "pubky://<author>/pub/pubky.app/posts/<post-id>",
  "post": {
    "kind": "event",
    "content": "{\"application_schema\":\"opaque to Nexus\"}",
    "parent": null,
    "embed": null,
    "attachments": null,
    "lock": null
  }
}
```

For deletion, the `post` value is `null`. Source strings retain case and spelling. Inventory includes legacy indexed posts; a source without a recorded projection revision uses `"0"`. Its parent is reconstructed from the existing reply relation until a new source PUT stores the raw parent.

Inventory orders by the existing globally unique Post ID and returns an opaque `next_cursor`, or `null` when enumeration is complete. It supports exact, case-sensitive comma-separated `kinds`, such as `event,calendar`, applied before pagination. Omitted kinds means all posts. Change replay is always unfiltered: consumers must observe deletions and transitions into or out of an interesting kind.

The default page limit is 50; valid limits are 1–100. A change page covers `(after, through]`; omitted `through` captures the current checkpoint for that request. Continue with the returned `cursor` and the same `through`. `caught_up: true` means the captured boundary has been reached, including any revision gaps. It does not mean the source will remain unchanged.

HTTP 410 means the epoch changed, the cursor expired, or a supplied cursor is beyond the current source checkpoint. Discard the incomplete scan/replay and restart inventory. Malformed revisions and invalid bounds return HTTP 400. Graph failures return a server error and must not be treated as empty results.

## Complete inventory and recovery

An inventory page is current graph state, not an immutable historical snapshot. A complete consumer must stage and reconcile it:

1. Capture the head epoch and start revision `S`.
2. Create an empty staging generation. Enumerate every inventory page with that epoch and `since=S`, using the returned cursor. Preserve the selected kinds for the whole scan.
3. Capture an end revision `E` in the same epoch after enumeration.
4. Replay all immutable changes in `(S,E]` into the staging generation. Apply complete source replacements, kind changes and deletions in revision order. Do not fetch their content from Redis or the homeserver.
5. Confirm the retained interval and epoch remain valid before atomically promoting the completed stage with checkpoint `E`. On an error, interruption or reset, discard the stage; never remove live records merely because an unfinished scan did not see them.
6. Continue replay from `E`. Track sync health and lag explicitly. An expired cursor requires the same complete staged rebuild.

Replaying after the scan corrects inserts, edits and deletions that occurred while the keyset enumeration was in progress. Consumers must not claim complete current coverage until the scan and replay have both succeeded. Current public visibility still requires the application's moderation policy; a historical payload is not authorization to serve a removed post.

## Retention and deployment limits

The current build retains a **10,000-revision window**, configured by the compile-time `RETAINED_REVISIONS` constant. It is not a time guarantee, and gaps can reduce the number of retained payloads. The minimum valid cursor advances in the source mutation transaction and never moves backwards, including after an administrative epoch reset establishes a higher floor.

At the 512 KiB custom-envelope maximum, 10,000 payloads approach **5 GiB before Neo4j indexes, property-store overhead, transaction logs, backups and the current graph**. Real disk use can be higher. A 100-item response can approach 50 MiB before encoding and runtime allocation overhead. Choose smaller consumer pages when memory or network capacity is limited. Disabling the HTTP token does **not** disable source recording or reduce retained history.

This is a bounded implementation, not evidence that the configured VPS can accommodate the worst case. Before enabling it in production, measure free disk, Neo4j heap/page-cache settings, existing graph size, write throughput, peak response memory, and whether a complete inventory finishes within the available revision window. If it cannot, increase capacity or change the retention design and benchmark it; repeatedly restarting an inventory is not a completeness solution.

## Restart, restore and rollback

Ordinary process restarts retain the checkpoint and immutable history. A completely empty graph creates a new random epoch. Restoring an older backup, changing indexed source data outside the shared mutation paths, or allowing an older binary to write requires a new epoch even if the restored numeric revision appears plausible.

For controlled maintenance, stop source writers and replication consumers, back up the graph, and rotate the checkpoint epoch under the same lock before resuming replication. Discard obsolete historical change records and set the minimum valid cursor to the current revision while preserving current Post source metadata and the numeric revision. Consumers then rebuild from inventory. There is no public reset endpoint; this is an explicit administrative operation.

To roll back the feature, stop the projection consumer and remove its public derived output, disable private API access, then restore the previous Nexus binaries. The added labels/properties/constraints are additive and can remain for a later controlled cleanup. Previous binaries do not maintain this log: rotate the epoch and rebuild consumers before enabling replication again. Rolling back does not retract notifications already delivered.

## Optional generic social text

A custom post can opt into native mentions with a field inside its opaque JSON `content`:

```json
{"social":{"version":1,"text":"Human text containing pk:<pubky-id> mentions"}}
```

The entire custom post still obeys its existing envelope bounds. `social.text` is limited to 69,632 UTF-8 bytes (68 KiB). Only version 1 with a string text value is accepted. Missing, malformed, future or oversized extensions supply no mentions. Nexus never scans unrelated metadata, organizer IDs, calendar URIs or arbitrary description fields.

Custom text recognizes the existing `pk:` and legacy `pubky` prefixes, deduplicates recipients, considers at most 32 recipients and excludes the author's self-mention. The author can consume one slot before exclusion. Known built-in kinds retain raw-text extraction and their existing lack of a custom recipient cap. Only the first non-author recipient is proactively ingested, preserving the native ingestion bound.

Every source PUT clears previous mention edges and rebuilds recipients from the current kind and content, including built-in edits, kind transitions, tombstones and retry/recovery. The resulting graph and cached recipient sets reflect the current text. Old notification history remains under native policy.

Mention notification insertion uses native Redis `ZADD NX` with the complete notification body. Repeating a write, recovering cache state, or removing and reintroducing the same mention cannot duplicate that body or move its original timestamp. The body includes post kind, so a kind transition can retain a distinct historical notification for the same post and recipient. This explicit behavior avoids an extra permanent deduplication store.

## Validation

The focused tests cover decimal revision precision and reset boundaries, disabled/missing/incorrect/valid bearer tokens, and generic mention extraction. An explicitly enabled watcher integration test covers graph/cache reconciliation and notification idempotence against disposable Neo4j and Redis services. The graph integration test covers immutable source payloads, rollback, concurrent writers, revision retention and the user-deletion guard. It requires a fresh isolated graph; do not run it against an existing instance.

The separate `retention_floor_is_monotonic_in_rollback_only_fixtures` common-library test verifies actual Cypher pruning before and after an epoch reset using random checkpoint IDs and epochs. It rolls back all writes and checks that no fixture nodes remain, so it does not require an empty test graph. Enable it explicitly with `NEXUS_PROJECTION_TEST_URI=127.0.0.1:17687` and the test runner's `--ignored` flag.

Run the service-free suites with:

```sh
cargo test -p nexus-common --lib post_projection::types
cargo test -p nexus-common --lib post::social
cargo test -p nexus-webapi --lib post_projection
cargo test -p nexus-watcher --lib post::mentions
cargo clippy -p nexus-common -p nexus-watcher -p nexus-webapi --lib -- -D warnings
```

For the ignored watcher integration test, supply disposable-service addresses in `EVENTKY_TEST_NEO4J_URI` and `EVENTKY_TEST_REDIS_URI`, then run `cargo test -p nexus-watcher --lib current_kind_mentions_reconcile_and_retry_without_timestamp_changes -- --ignored`. The test creates random fixtures and does not clear an existing graph.

The real homeserver acceptance fixture additionally starts a local Pubky DHT, relay and homeserver and sends raw `event`/`calendar` envelopes through homeserver storage and the watcher. It checks native comments, tags, reposts, bookmarks, exact kind/content in graph and cache, kind-only and content edits, removals, and immutable source history. It uses explicit `WatcherTest::setup_with_stack` configuration and does not clear other graph data.

Provide dedicated Neo4j/Redis services and PostgreSQL with permission to create temporary databases. Set `EVENTKY_TEST_NEO4J_URI`, `EVENTKY_TEST_REDIS_URI`, and `TEST_PUBKY_CONNECTION_STRING`; the PostgreSQL URL must include `?pubky-test=true` (or the equivalent additional query parameter) to select an ephemeral database. Local TCP/UDP binds are required. Then run:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p nexus-watcher --test mod \
  event_processor::posts::eventky_acceptance::event_and_calendar_homeserver_social_lifecycle \
  -- --exact --ignored --test-threads=1
```

The test helpers retain their default configuration for existing tests. This acceptance fixture requires explicit service addresses and creates random test identities; run it only against disposable local services.

Passing these correctness tests does not replace deployment sizing or source-write throughput testing.
