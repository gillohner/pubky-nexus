//! Independent, content-free app-post version changes. Never writes the resource feed.

use crate::db::graph::{GraphError, Query};
use crate::db::kv::{RedisError, RedisResult};
use crate::db::{get_neo4j_graph, get_redis_conn, release_lock, try_acquire_lock};
use crate::models::app_post::{AppPost, AppPostUri};
use crate::models::error::{ModelError, ModelResult};
use deadpool_redis::redis::{self, AsyncCommands, Script};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;
use utoipa::ToSchema;

pub const APP_POST_CHANGES_KEY: &str = "Event:AppPostChanges";
const EPOCH_KEY: &str = "Event:AppPostChanges:Epoch";
const VERSIONS_KEY: &str = "Event:AppPostChanges:Versions";
const SEED_LOCK: &str = "Event:AppPostChanges:SeedLock";
const SEED_TTL: u64 = 3660;
const BATCH_SIZE: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AppPostChangeOp {
    Put,
    Del,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AppPostChange {
    pub op: AppPostChangeOp,
    pub author: String,
    /// Versionless post path. Logical identity is (author, post).
    pub post: String,
    /// Exact stored file path, including an optional slug. A version DEL removes
    /// only this file from the consumer's version set. Null means whole-post DEL.
    /// PUT always carries a version. N3 emits file DELs, never infers a whole-post DEL.
    #[schema(required = true)]
    pub version: Option<String>,
    pub namespace: String,
    /// Namespace-owned kind; never a resource-reference kind.
    pub post_kind: String,
    /// Acceptance/deletion time in Unix milliseconds.
    pub at: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AppPostChangesPage {
    pub lines: Vec<AppPostChange>,
    pub next_cursor: u64,
}

// A random 48-bit initial counter avoids reusing a feed identity after Redis is
// cleared. INCR on reseed remains exact in Redis Lua's numeric representation.
fn epoch_seed() -> u64 {
    let token = pubky::Keypair::random().public_key().to_z32();
    let digest = blake3::hash(token.as_bytes());
    let mut bytes = [0; 8];
    bytes[..6].copy_from_slice(&digest.as_bytes()[..6]);
    u64::from_le_bytes(bytes)
}

fn pending_key(author: &str, path: &str) -> String {
    format!("Event:AppPostChanges:PendingDelete:{author}:{path}")
}

static READ: LazyLock<Script> = LazyLock::new(|| {
    Script::new(
        r"
    return {redis.call('GET', KEYS[2]) or false,
            redis.call('LLEN', KEYS[1]),
            redis.call('LRANGE', KEYS[1], ARGV[1], ARGV[2])}
",
    )
});

impl AppPostChange {
    pub fn new(path: &AppPostUri, post_kind: String, op: AppPostChangeOp) -> Self {
        Self {
            op,
            author: path.author.to_string(),
            post: path.path(),
            version: path.version.as_ref().map(|_| path.version_path()),
            namespace: path.namespace.clone(),
            post_kind,
            at: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn from_post(post: AppPost) -> Self {
        let version = post
            .version_uri
            .strip_prefix(&format!("pubky://{}", post.author))
            .expect("AppPost version URI belongs to its author")
            .to_owned();
        Self {
            op: AppPostChangeOp::Put,
            author: post.author,
            post: post.path,
            version: Some(version),
            namespace: post.namespace,
            post_kind: post.kind,
            at: chrono::Utc::now().timestamp_millis(),
        }
    }

    fn json(&self) -> RedisResult<String> {
        serde_json::to_string(self).map_err(|e| RedisError::SerializationFailed(Box::new(e)))
    }

    /// Graph commit precedes this atomic append and version metadata update.
    /// Retry may repeat a PUT; it must not lose the line after a graph-only commit.
    pub async fn store(&self) -> RedisResult<()> {
        let version = self
            .version
            .as_ref()
            .filter(|_| self.op == AppPostChangeOp::Put)
            .ok_or_else(|| RedisError::SerializationFailed("PUT requires a version".into()))?;
        static APPEND: LazyLock<Script> = LazyLock::new(|| {
            Script::new(
                r"
            redis.call('SETNX', KEYS[2], ARGV[2])
            redis.call('RPUSH', KEYS[1], ARGV[1])
            redis.call('HSET', KEYS[3], ARGV[3], ARGV[1])
            redis.call('DEL', KEYS[4], KEYS[5])
        ",
            )
        });
        APPEND
            .key(APP_POST_CHANGES_KEY)
            .key(EPOCH_KEY)
            .key(VERSIONS_KEY)
            .key(pending_key(&self.author, version))
            .key(pending_key(&self.author, &self.post))
            .arg(self.json()?)
            .arg(epoch_seed())
            .arg(format!("pubky://{}{version}", self.author))
            .invoke_async::<()>(&mut get_redis_conn().await?)
            .await?;
        Ok(())
    }

    /// Metadata for every accepted version, including feed-only and selected social posts.
    /// Kept until that version is deleted, even after N3 removes the logical graph node.
    pub async fn version_record(path: &AppPostUri) -> RedisResult<Option<Self>> {
        let raw: Option<String> = get_redis_conn()
            .await?
            .hget(VERSIONS_KEY, path.version_uri())
            .await?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|e| RedisError::DeserializationFailed(Box::new(e)))
        })
        .transpose()
    }

    pub async fn prepare_delete(&self) -> RedisResult<()> {
        let _: bool = get_redis_conn()
            .await?
            .set_nx(
                pending_key(&self.author, self.version.as_deref().unwrap_or(&self.post)),
                self.json()?,
            )
            .await?;
        Ok(())
    }

    pub async fn prepare_version_delete(path: &AppPostUri) -> RedisResult<()> {
        if let Some(mut line) = Self::version_record(path).await? {
            line.op = AppPostChangeOp::Del;
            line.at = chrono::Utc::now().timestamp_millis();
            line.prepare_delete().await?;
        }
        Ok(())
    }

    /// Pending details survive the graph delete. A completed version DEL is idempotent.
    /// Null-version whole-post DELs clear all version records for that logical identity.
    pub async fn finish_delete(path: &AppPostUri) -> RedisResult<()> {
        static FINISH: LazyLock<Script> = LazyLock::new(|| {
            Script::new(
                r"
            local raw = redis.call('GET', KEYS[4])
            if raw then
                local line = cjson.decode(raw)
                redis.call('SETNX', KEYS[2], ARGV[1])
                redis.call('RPUSH', KEYS[1], raw)
                redis.call('DEL', KEYS[4])
                if line.version ~= cjson.null then
                    redis.call('HDEL', KEYS[3], 'pubky://' .. line.author .. line.version)
                else
                    local prefix = 'pubky://' .. line.author .. line.post .. '/'
                    local cursor = '0'
                    repeat
                        local page = redis.call('HSCAN', KEYS[3], cursor)
                        cursor = page[1]
                        for i = 1, #page[2], 2 do
                            if string.sub(page[2][i], 1, #prefix) == prefix then
                                redis.call('HDEL', KEYS[3], page[2][i])
                            end
                        end
                    until cursor == '0'
                end
            end
        ",
            )
        });
        FINISH
            .key(APP_POST_CHANGES_KEY)
            .key(EPOCH_KEY)
            .key(VERSIONS_KEY)
            .key(pending_key(path.author.as_ref(), &path.version_path()))
            .arg(epoch_seed())
            .invoke_async::<()>(&mut get_redis_conn().await?)
            .await?;
        Ok(())
    }

    /// Atomically read the page, epoch and length from one list generation.
    /// No SETNX or other write occurs here, including on an empty instance.
    pub async fn get_from_index(
        cursor: u64,
        limit: usize,
    ) -> RedisResult<(AppPostChangesPage, Option<String>, u64)> {
        let start = cursor.min(i64::MAX as u64);
        let end = start
            .saturating_add(limit.saturating_sub(1) as u64)
            .min(i64::MAX as u64);
        let mut conn = get_redis_conn().await?;
        let (epoch, length, raw): (Option<String>, u64, Vec<String>) = READ
            .key(APP_POST_CHANGES_KEY)
            .key(EPOCH_KEY)
            .arg(if limit == 0 { 1 } else { start })
            .arg(if limit == 0 { 0 } else { end })
            .invoke_async(&mut conn)
            .await?;
        let lines = raw
            .into_iter()
            .map(|line| {
                serde_json::from_str(&line)
                    .map_err(|e| RedisError::DeserializationFailed(Box::new(e)))
            })
            .collect::<RedisResult<Vec<_>>>()?;
        let next_cursor = cursor.saturating_add(lines.len() as u64);
        Ok((AppPostChangesPage { lines, next_cursor }, epoch, length))
    }

    /// Rebuild from version metadata and graph fallback, then atomically publish with a new epoch.
    /// The old list's suffix is copied after the snapshot, preserving concurrent
    /// watcher appends (duplicates are resolved by the consumer's replay key).
    pub async fn reseed() -> ModelResult<u64> {
        let token = pubky::Keypair::random().public_key().to_z32();
        if !try_acquire_lock(SEED_LOCK, &token, SEED_TTL).await? {
            return Err(ModelError::from_generic(
                "App post changes seed already running",
            ));
        }
        let temp = format!("{APP_POST_CHANGES_KEY}:Seed:{token}");
        let result = tokio::time::timeout(Duration::from_secs(3600), seed_inner(&token, &temp))
            .await
            .map_err(ModelError::from_generic)
            .and_then(|r| r);
        // Aborted processes leave only expiring temporary data and the old feed.
        if let Ok(mut conn) = get_redis_conn().await {
            let _ = conn.del::<_, ()>(&temp).await;
        }
        release_lock(SEED_LOCK, &token).await?;
        result
    }
}

async fn seed_inner(token: &str, temp: &str) -> ModelResult<u64> {
    let mut conn = get_redis_conn().await?;
    let start: u64 = conn
        .llen(APP_POST_CHANGES_KEY)
        .await
        .map_err(RedisError::from)?;
    // Graph fallback recovers durable app heads after Redis loss or graph-only commit.
    // Accepted version metadata follows it, then the live suffix wins over both.
    let graph = get_neo4j_graph()?;
    let mut rows = graph
        .execute(Query::new(
            "app_post_changes_seed",
            r"
        CALL {
            MATCH (p:AppPost) RETURN p
            UNION ALL
            MATCH (s:Post) WHERE s.post_feed_version IS NOT NULL AND NOT coalesce(s.deleted, false)
            RETURN {uri: s.uri, version_uri: 'pubky://' + s.author + s.post_feed_version,
                    id: s.id, author: s.author, path: '/pub/social/v1/posts/' + s.id,
                    namespace: 'social', kind: s.post_feed_kind, indexed_at: s.post_feed_at,
                    parent: null, embed: null} AS p
        }
        RETURN p ORDER BY p.indexed_at, p.uri
    ",
        ))
        .await
        .map_err(GraphError::from)?;
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    while let Some(row) = rows.try_next().await.map_err(GraphError::from)? {
        let post: AppPost = row.get("p")?;
        if !conn
            .hexists::<_, _, bool>(VERSIONS_KEY, &post.version_uri)
            .await
            .map_err(RedisError::from)?
        {
            let at = post.indexed_at;
            let mut line = AppPostChange::from_post(post);
            line.at = at;
            batch.push(line.json()?);
            if batch.len() == BATCH_SIZE {
                write_seed_batch(temp, &batch).await?;
                batch.clear();
            }
        }
    }
    if !batch.is_empty() {
        write_seed_batch(temp, &batch).await?;
    }
    let mut cursor = 0u64;
    loop {
        let (next, records): (u64, Vec<(String, String)>) = redis::cmd("HSCAN")
            .arg(VERSIONS_KEY)
            .arg(cursor)
            .arg("COUNT")
            .arg(BATCH_SIZE)
            .query_async(&mut conn)
            .await
            .map_err(RedisError::from)?;
        let lines: Vec<String> = records.into_iter().map(|(_, raw)| raw).collect();
        if !lines.is_empty() {
            write_seed_batch(temp, &lines).await?;
        }
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    publish_seed(token, temp, start).await.map_err(Into::into)
}

async fn write_seed_batch(temp: &str, batch: &[String]) -> RedisResult<()> {
    let mut conn = get_redis_conn().await?;
    redis::pipe()
        .atomic()
        .cmd("RPUSH")
        .arg(temp)
        .arg(batch)
        .ignore()
        .cmd("EXPIRE")
        .arg(temp)
        .arg(SEED_TTL)
        .ignore()
        .query_async::<()>(&mut conn)
        .await?;
    Ok(())
}

async fn publish_seed(token: &str, temp: &str, start: u64) -> RedisResult<u64> {
    static PUBLISH: LazyLock<Script> = LazyLock::new(|| {
        Script::new(
            r"
        if redis.call('GET', KEYS[4]) ~= ARGV[1] then
            return redis.error_reply('App post changes seed lease expired')
        end
        local length = redis.call('LLEN', KEYS[1])
        if length < tonumber(ARGV[2]) then
            return redis.error_reply('App post changes feed was truncated during seed')
        end
        for offset = tonumber(ARGV[2]), length - 1, 1000 do
            local tail = redis.call('LRANGE', KEYS[1], offset, math.min(offset + 999, length - 1))
            redis.call('RPUSH', KEYS[3], unpack(tail))
        end
        if redis.call('EXISTS', KEYS[3]) == 1 then
            redis.call('RENAME', KEYS[3], KEYS[1])
            redis.call('PERSIST', KEYS[1])
        else
            redis.call('DEL', KEYS[1])
        end
        redis.call('SETNX', KEYS[2], ARGV[3])
        redis.call('INCR', KEYS[2])
        return redis.call('LLEN', KEYS[1])
    ",
        )
    });
    let mut conn = get_redis_conn().await?;
    Ok(PUBLISH
        .key(APP_POST_CHANGES_KEY)
        .key(EPOCH_KEY)
        .key(temp)
        .key(SEED_LOCK)
        .arg(token)
        .arg(start)
        .arg(epoch_seed())
        .invoke_async(&mut conn)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StackConfig, StackManager};

    fn path(author: &str, namespace: &str, id: &str) -> AppPostUri {
        AppPostUri::parse(&format!(
            "pubky://{author}/pub/{namespace}/v1/posts/{id}/{id}.json"
        ))
        .unwrap()
    }

    // Global feed replacements require nextest exclusivity.
    #[tokio_shared_rt::test(shared)]
    async fn live_seed_versions_graph_fallback_and_delete_scope(
    ) -> Result<(), crate::types::DynError> {
        StackManager::setup(&StackConfig::default()).await?;
        let mut conn = get_redis_conn().await?;
        let _: () = conn
            .del(&[APP_POST_CHANGES_KEY, EPOCH_KEY, VERSIONS_KEY])
            .await?;
        let empty = AppPostChange::get_from_index(0, 1000).await?;
        assert!(empty.0.lines.is_empty());
        assert_eq!((empty.0.next_cursor, empty.1, empty.2), (0, None, 0));
        assert!(!conn.exists::<_, bool>(EPOCH_KEY).await?);
        assert!(!conn.exists::<_, bool>(APP_POST_CHANGES_KEY).await?);
        let resource_before: Vec<String> = conn.lrange("Event:ResourceChanges", 0, -1).await?;
        let author = pubky::Keypair::random().public_key().to_z32();
        let graph = get_neo4j_graph()?;
        graph
            .run(
                Query::new("app_feed_seed_author", "CREATE (:User {id: $author})")
                    .param("author", author.clone()),
            )
            .await?;
        let durable = path(&author, "events", "0032SSN7Q4EVG");
        let input = durable
            .read(br#"{"kind":"availability","content":"opaque"}"#)
            .unwrap();
        assert!(AppPost::new(&durable, input).put().await?); // graph commit, no append yet
        let first = path(&author, "music", "0032SSN7Q4EVJ");
        let mut second = first.clone();
        second.version = Some("0032SSN7Q4EVM".into());
        second.label = Some("now-playing".into());
        for version in [&first, &second] {
            AppPostChange::new(version, "listening".into(), AppPostChangeOp::Put)
                .store()
                .await?;
        }
        let (_, epoch, _) = AppPostChange::get_from_index(0, 0).await?;
        AppPostChange::reseed().await?;
        let (page, next_epoch, seed_length) = AppPostChange::get_from_index(0, 1000).await?;
        assert_ne!(epoch, next_epoch);
        let own: Vec<_> = page
            .lines
            .iter()
            .filter(|line| line.author == author)
            .collect();
        assert_eq!(own.len(), 3);
        assert_eq!(own[0].post_kind, "availability");
        assert_eq!(
            own[0].version.as_deref(),
            Some(durable.version_path().as_str())
        );
        // A version deletion preserves the other version's metadata, including its own kind.
        AppPostChange::prepare_version_delete(&first).await?;
        AppPostChange::finish_delete(&first).await?;
        AppPostChange::finish_delete(&first).await?;
        let (page, _, length) = AppPostChange::get_from_index(seed_length, 10).await?;
        assert_eq!(length, seed_length + 1);
        assert_eq!(page.lines.len(), 1);
        assert_eq!(
            page.lines[0].version.as_deref(),
            Some(first.version_path().as_str())
        );
        assert!(AppPostChange::version_record(&first).await?.is_none());
        assert!(AppPostChange::version_record(&second).await?.is_some());
        // The contract represents an explicit whole-post delete with null, never with
        // the last known version. N3 storage events themselves only emit file deletes.
        let mut logical = first.clone();
        logical.version = None;
        logical.label = None;
        let whole = AppPostChange::new(&logical, "listening".into(), AppPostChangeOp::Del);
        assert_eq!(
            serde_json::to_value(&whole)?["version"],
            serde_json::Value::Null
        );
        whole.prepare_delete().await?;
        AppPostChange::finish_delete(&logical).await?;
        assert!(AppPostChange::version_record(&second).await?.is_none());
        assert_eq!(
            AppPostChange::get_from_index(seed_length + 1, 10)
                .await?
                .0
                .lines,
            [whole]
        );
        assert!(AppPostChange::get_from_index(u64::MAX, 1000)
            .await?
            .0
            .lines
            .is_empty());
        assert_eq!(
            AppPostChange::get_from_index(u64::MAX, 1000)
                .await?
                .0
                .next_cursor,
            u64::MAX
        );
        assert!(AppPostChange::get_from_index(0, 0)
            .await?
            .0
            .lines
            .is_empty());
        assert_eq!(
            conn.lrange::<_, Vec<String>>("Event:ResourceChanges", 0, -1)
                .await?,
            resource_before
        );
        AppPost::delete(&durable.uri()).await?;
        graph
            .run(
                Query::new(
                    "app_feed_seed_cleanup",
                    "MATCH (u:User {id: $author}) DETACH DELETE u",
                )
                .param("author", author),
            )
            .await?;
        Ok(())
    }

    #[tokio_shared_rt::test(shared)]
    async fn live_seed_recovers_selected_social_head_without_redis_metadata(
    ) -> Result<(), crate::types::DynError> {
        StackManager::setup(&StackConfig::default()).await?;
        let author = pubky::Keypair::random().public_key().to_z32();
        let path = AppPostUri::parse_social(&format!(
            "pubky://{author}/pub/social/v1/posts/0032SSN7Q4EVG/0032SSN7Q4EVJ.json"
        ))
        .unwrap();
        let graph = get_neo4j_graph()?;
        graph
            .run(
                Query::new(
                    "social_feed_seed_test",
                    r"
            CREATE (:Post {author: $author, id: $id, uri: $uri, post_feed_version: $version,
                           post_feed_kind: 'event', post_feed_at: 1234})
        ",
                )
                .param("author", author.clone())
                .param("id", path.id.clone())
                .param("uri", path.uri())
                .param("version", path.version_path()),
            )
            .await?;
        AppPostChange::reseed().await?;
        let lines = AppPostChange::get_from_index(0, 1000).await?.0.lines;
        let line = lines.iter().find(|line| line.author == author).unwrap();
        assert_eq!(line.namespace, "social");
        assert_eq!(line.post_kind, "event");
        assert_eq!(line.version.as_deref(), Some(path.version_path().as_str()));
        assert_eq!(line.at, 1234);
        assert!(AppPostChange::version_record(&path).await?.is_none());
        graph
            .run(
                Query::new(
                    "social_feed_seed_cleanup",
                    "MATCH (p:Post {author: $author}) DELETE p",
                )
                .param("author", author),
            )
            .await?;
        Ok(())
    }

    #[tokio_shared_rt::test(shared)]
    async fn live_seed_preserves_interleaved_appends_and_rejects_expired_lease(
    ) -> Result<(), crate::types::DynError> {
        StackManager::setup(&StackConfig::default()).await?;
        let author = pubky::Keypair::random().public_key().to_z32();
        let version = path(&author, "maps", "0032SSN7Q4EVG");
        let line = AppPostChange::new(&version, "route".into(), AppPostChangeOp::Put);
        let mut conn = get_redis_conn().await?;
        let start: u64 = conn.llen(APP_POST_CHANGES_KEY).await?;
        let temp = format!("{APP_POST_CHANGES_KEY}:Seed:test-{author}");
        assert!(try_acquire_lock(SEED_LOCK, &author, SEED_TTL).await?);
        write_seed_batch(&temp, &[line.json()?]).await?;
        // Writes made after snapshot start must follow it, including a duplicate PUT
        // and its DEL; neither can disappear when the new generation is published.
        line.store().await?;
        line.store().await?;
        AppPostChange::prepare_version_delete(&version).await?;
        AppPostChange::finish_delete(&version).await?;
        assert_eq!(publish_seed(&author, &temp, start).await?, 4);
        release_lock(SEED_LOCK, &author).await?;
        let page = AppPostChange::get_from_index(0, 10).await?.0;
        assert_eq!(page.lines[..3], [line.clone(), line.clone(), line]);
        assert_eq!(page.lines[3].op, AppPostChangeOp::Del);
        let epoch = AppPostChange::get_from_index(0, 0).await?.1;
        assert!(publish_seed(&author, &temp, 0).await.is_err());
        assert_eq!(AppPostChange::get_from_index(0, 0).await?.1, epoch);
        Ok(())
    }
}
