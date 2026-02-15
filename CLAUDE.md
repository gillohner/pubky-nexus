# Pubky Nexus — Social Graph Indexer

Rust service bridging Pubky homeservers and social app clients.
Watches homeserver events → builds Neo4j social graph + Redis cache → serves REST API.

## Architecture

Cargo workspace with 4 crates:
- `nexus-common/` — Shared models, DB connectors, queries (library crate)
- `nexus-watcher/` — Event listener that indexes homeserver events into DBs
- `nexus-webapi/` — REST API server (Swagger UI at /swagger-ui)
- `nexusd/` — Binary orchestrator, DB migrations, CLI commands

### Data Stores
- **Neo4j** — Social graph (users, follows, posts, events, relationships). Cypher queries.
- **Redis** — Cache layer for hot queries (<1ms response). Most requests served from here.
- **PostgreSQL** — Used by webapi for certain query patterns

## Commands
- `cargo run -p nexusd` — Run full service (defaults to $HOME/.pubky-nexus/config.toml)
- `cargo run -p nexusd -- watcher` — Run watcher only
- `cargo run -p nexusd -- api` — Run API only
- `cargo run -p nexusd -- db mock` — Load test mock data into Neo4j + Redis
- `cargo run -p nexusd -- db clear` — Clear databases
- `cargo run -p nexusd -- db migration run` — Run pending migrations
- `cargo nextest run -p nexus-common --no-fail-fast` — Test common crate
- `cargo nextest run -p nexus-watcher --no-fail-fast` — Test watcher
- `export TEST_PUBKY_CONNECTION_STRING=postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true && cargo nextest run -p nexus-webapi --no-fail-fast` — Test webapi
- `cargo bench -p nexus-webapi` — Run benchmarks
- `cd docker && docker compose up -d` — Start Neo4j + Redis + Postgres

## Code Patterns
- Models in `nexus-common` must align with `pubky-app-specs` Rust structs
- Redis keys follow consistent naming — match existing patterns in the codebase
- Neo4j queries use Cypher — see `nexus-common/src/` for query patterns
- Watcher handlers translate homeserver events into graph operations
- API endpoints return JSON — Swagger at /swagger-ui/ for response schemas

## IMPORTANT Rules
- NEVER modify pubky-app-specs models here — changes originate in pubky-app-specs repo
- When adding a new data type, update ALL: watcher (indexing), common (models/queries), webapi (endpoints)
- Redis cache invalidation MUST match any Neo4j write — stale cache is the #1 bug source
- Test data lives in `docker/test-graph/mocks/` — update when adding new model fields
- Migration phases: dual_write → backfill → cutover → cleanup (see README)
- Always create a feature branch: `git checkout -b feat/<description>`
- After changes, verify: `cargo nextest run --no-fail-fast`
