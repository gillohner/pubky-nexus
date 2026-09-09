# Opt-in primary user indexing

`watcher.primary_user_indexing` defaults to `false`. Staging can enable it with
`monitored_homeservers_limit = 0`: the primary homeserver is a separate target,
not an external target. Homeserver blacklists still apply. Explicitly ingest a
user through the existing ingestion API to create its per-user cursor at zero.
Only primary users with an existing cursor are delegated; missing cursors are
not inferred from historical graph membership. New zero cursors are processed
before established cursors.

The global stream continues processing its full history and advancing its own
cursor. For delegated users, the ordered per-user stream owns event effects
from its own cursor; the global stream does not also apply them. Neither cursor
is reset. A shared in-process guard serializes handoff with in-flight global
work and queued retries. Delegated transient failures retain the per-user cursor
instead of allowing newer events to overtake a queued retry. Old global retries
for delegated users are removed because the per-user stream owns that history.
Primary processor timeouts abort and join their task before another poll starts.
Run one watcher process for these stores: the guard is not a distributed lock.

## Sticky setting and rollback limitation

**Keep the setting enabled once users are delegated.** Disabling it or rolling
back to an older image while global history trails delegated users can replay
old deletes after newer per-user writes. There is no automatic drain or safe
rollback protocol in this opt-in feature. A current global cursor alone does
not establish a safe boundary: it needs a known homeserver head and verified
per-user ownership history. Changing ownership requires an explicit operator
migration with processing stopped and independently verified boundaries; this
document does not prescribe an unsafe cursor reset or cursor skip. Startup emits
a warning whenever this feature is enabled. Default deployments remain unchanged.

## Private projection quota

`api.rate_limit.projection_bucket` defaults to 120 requests/minute, burst 10.
Authenticated projection endpoints use this independent quota, leaving public
expensive/default quotas unchanged. Authorization runs before quota accounting;
unauthorized requests cannot consume the replication allowance. The bucket uses
the connection peer address rather than forwarded headers. Multiple consumers
behind one peer share its allowance. Existing token authorization and private
network/proxy restrictions remain required.

## Validation

No-database unit tests cover explicit cursor selection, handoff ordering, timeout
cancellation, legacy configuration defaults, and authenticated quota isolation.
Redis integration tests additionally cover global cursor advancement during
delegation, ordered per-user retry retention, target selection/blacklisting, and
removal of stale primary retries. These integration tests require isolated test
stores and must not run against production Redis or Neo4j.

The three primary ownership integration regressions are ignored by default,
because their setup refuses shared/default database endpoints. After provisioning
disposable stores and forwarding their ports locally, run each filter explicitly:

```sh
export NEXUS_PRIMARY_TEST_NEO4J_URI=bolt://127.0.0.1:17687
export NEXUS_PRIMARY_TEST_REDIS_URI=redis://127.0.0.1:16379
cargo test -p nexus-watcher --test mod tracked_primary_user_is_delegated_without_resetting_global_or_user_history -- --ignored
cargo test -p nexus-watcher --test mod primary_user_lane_keeps_failed_event_at_cursor_without_queued_retry -- --ignored
cargo test -p nexus-watcher --test mod old_primary_retry_is_delegated_before_it_can_delete_newer_user_state -- --ignored
```

Both environment variables are required and only these exact loopback endpoints
are accepted. Neo4j must have authentication disabled. Run the separate empty-graph
projection concurrency fixture first: the primary integration tests create graph
nodes, and the concurrency fixture deliberately refuses a nonempty graph.
