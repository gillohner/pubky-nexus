//! Fork-only self references for app and selected social post versions.
//! The list, epoch, pending-delete journal and publication rules belong to the
//! resource feed. Only per-version recovery metadata is specific to posts.

use super::resource_changes::{
    write_seed_batch, ResourceChange, ResourceChangeKind, ResourceChangeOp, BATCH_SIZE,
};
use crate::db::graph::{GraphError, Query};
use crate::db::kv::{RedisError, RedisResult};
use crate::db::{get_neo4j_graph, get_redis_conn};
use crate::models::app_post::AppPostUri;
use crate::models::error::{ModelError, ModelResult};
use crate::universal_tag::normalize::{normalize_uri, resource_id};
use deadpool_redis::redis::{self, AsyncCommands};
use futures::TryStreamExt;

pub(super) const POST_VERSIONS_KEY: &str = "Event:ResourceChanges:PostVersions";

impl ResourceChange {
    /// The object names itself. The logical URI is normalized and hashed exactly
    /// like any Resource; the replay key still names the actual version file.
    pub fn post(path: &AppPostUri, op: ResourceChangeOp) -> ModelResult<Self> {
        if path.version.is_none() {
            return Err(ModelError::from_generic(
                "A post reference requires a stored version",
            ));
        }
        let (uri, _) = normalize_uri(&path.uri()).map_err(ModelError::from_generic)?;
        Ok(Self {
            op,
            resource_id: resource_id(&uri),
            uri,
            kind: ResourceChangeKind::Post,
            author: path.author.to_string(),
            label: None,
            object_path: path.version_path(),
            at: chrono::Utc::now().timestamp_millis(),
        })
    }

    /// Every accepted version survives here until its DEL, including feed-only
    /// versions and earlier edits after N3 removes the logical graph metadata.
    pub async fn version_record(path: &AppPostUri) -> RedisResult<Option<Self>> {
        let raw: Option<String> = get_redis_conn()
            .await?
            .hget(POST_VERSIONS_KEY, path.version_uri())
            .await?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|e| RedisError::DeserializationFailed(Box::new(e)))
        })
        .transpose()
    }

    /// Selected social DELs require evidence that this version was selected.
    pub async fn prepare_version_delete(path: &AppPostUri) -> RedisResult<()> {
        if let Some(mut line) = Self::version_record(path).await? {
            line.op = ResourceChangeOp::Del;
            line.at = chrono::Utc::now().timestamp_millis();
            line.prepare_delete().await?;
        }
        Ok(())
    }
}

pub(super) async fn seed_post_references(temp: &str) -> ModelResult<()> {
    let mut conn = get_redis_conn().await?;
    // Recover a graph-only commit or current heads after Redis loss. Historical
    // and feed-only versions come from metadata; live appends follow the snapshot.
    let mut rows = get_neo4j_graph()?
        .execute(Query::new(
            "post_references_seed",
            r"
        CALL {
            MATCH (p:Resource) WHERE p.namespace IS NOT NULL AND p.version_uri IS NOT NULL
            RETURN p.version_uri AS version_uri, p.app_indexed_at AS at
            UNION ALL
            MATCH (s:Post) WHERE s.post_feed_version IS NOT NULL AND NOT coalesce(s.deleted, false)
            RETURN 'pubky://' + s.author + s.post_feed_version AS version_uri, s.post_feed_at AS at
        }
        RETURN version_uri, at ORDER BY at, version_uri
    ",
        ))
        .await
        .map_err(GraphError::from)?;
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    while let Some(row) = rows.try_next().await.map_err(GraphError::from)? {
        let version_uri: String = row.get("version_uri")?;
        if !conn
            .hexists::<_, _, bool>(POST_VERSIONS_KEY, &version_uri)
            .await
            .map_err(RedisError::from)?
        {
            let path = AppPostUri::parse(&version_uri)
                .or_else(|| AppPostUri::parse_social(&version_uri))
                .ok_or_else(|| ModelError::from_generic("Invalid stored post version URI"))?;
            let mut line = ResourceChange::post(&path, ResourceChangeOp::Put)?;
            line.at = row.get("at")?;
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
            .arg(POST_VERSIONS_KEY)
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::resource_changes::{
        publish_seed, EPOCH_KEY, RESOURCE_CHANGES_KEY, SEED_LOCK, SEED_TTL,
    };
    use super::*;
    use crate::db::{release_lock, try_acquire_lock};
    use crate::models::app_post::AppPost;
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
            .del(&[RESOURCE_CHANGES_KEY, EPOCH_KEY, POST_VERSIONS_KEY])
            .await?;
        let empty = ResourceChange::get_from_index(0, 1000).await?;
        assert!(empty.0.lines.is_empty());
        assert_eq!((empty.0.next_cursor, empty.1, empty.2), (0, None, 0));
        assert!(!conn.exists::<_, bool>(EPOCH_KEY).await?);
        assert!(!conn.exists::<_, bool>(RESOURCE_CHANGES_KEY).await?);
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
            ResourceChange::post(version, ResourceChangeOp::Put)?
                .store()
                .await?;
        }
        let (_, epoch, _) = ResourceChange::get_from_index(0, 0).await?;
        ResourceChange::reseed().await?;
        let (page, next_epoch, seed_length) = ResourceChange::get_from_index(0, 1000).await?;
        assert_ne!(epoch, next_epoch);
        let own: Vec<_> = page
            .lines
            .iter()
            .filter(|line| line.author == author)
            .collect();
        assert_eq!(own.len(), 3);
        assert_eq!(own[0].uri, durable.uri());
        assert_eq!(own[0].object_path, durable.version_path());
        // A version deletion preserves the other version's metadata, including its own kind.
        ResourceChange::prepare_version_delete(&first).await?;
        ResourceChange::finish_delete(first.author.as_ref(), &first.version_uri()).await?;
        ResourceChange::finish_delete(first.author.as_ref(), &first.version_uri()).await?;
        let (page, _, length) = ResourceChange::get_from_index(seed_length, 10).await?;
        assert_eq!(length, seed_length + 1);
        assert_eq!(page.lines.len(), 1);
        assert_eq!(page.lines[0].object_path, first.version_path());
        assert!(ResourceChange::version_record(&first).await?.is_none());
        assert!(ResourceChange::version_record(&second).await?.is_some());
        // A second file DEL names that file only. No synthetic whole-post deletion.
        ResourceChange::prepare_version_delete(&second).await?;
        ResourceChange::finish_delete(second.author.as_ref(), &second.version_uri()).await?;
        assert!(ResourceChange::version_record(&second).await?.is_none());
        let mut logical = first.clone();
        logical.version = None;
        assert!(ResourceChange::post(&logical, ResourceChangeOp::Del).is_err());
        let last = ResourceChange::get_from_index(seed_length + 1, 10)
            .await?
            .0
            .lines;
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].object_path, second.version_path());
        assert_eq!(last[0].uri, first.uri());
        assert!(ResourceChange::get_from_index(u64::MAX, 1000)
            .await?
            .0
            .lines
            .is_empty());
        assert_eq!(
            ResourceChange::get_from_index(u64::MAX, 1000)
                .await?
                .0
                .next_cursor,
            u64::MAX
        );
        assert!(ResourceChange::get_from_index(0, 0)
            .await?
            .0
            .lines
            .is_empty());
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
        ResourceChange::reseed().await?;
        let lines = ResourceChange::get_from_index(0, 1000).await?.0.lines;
        let line = lines.iter().find(|line| line.author == author).unwrap();
        assert_eq!(line.uri, path.uri());
        assert_eq!(line.kind, ResourceChangeKind::Post);
        assert_eq!(line.object_path, path.version_path());
        assert_eq!(line.at, 1234);
        assert!(ResourceChange::version_record(&path).await?.is_none());
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
        let line = ResourceChange::post(&version, ResourceChangeOp::Put)?;
        let mut conn = get_redis_conn().await?;
        let start: u64 = conn.llen(RESOURCE_CHANGES_KEY).await?;
        let temp = format!("{RESOURCE_CHANGES_KEY}:Seed:test-{author}");
        assert!(try_acquire_lock(SEED_LOCK, &author, SEED_TTL).await?);
        write_seed_batch(&temp, &[line.json()?]).await?;
        // Writes made after snapshot start must follow it, including a duplicate PUT
        // and its DEL; neither can disappear when the new generation is published.
        line.store().await?;
        line.store().await?;
        ResourceChange::prepare_version_delete(&version).await?;
        ResourceChange::finish_delete(version.author.as_ref(), &version.version_uri()).await?;
        assert_eq!(publish_seed(&author, &temp, start).await?, 4);
        release_lock(SEED_LOCK, &author).await?;
        let page = ResourceChange::get_from_index(0, 10).await?.0;
        assert_eq!(page.lines[..3], [line.clone(), line.clone(), line]);
        assert_eq!(page.lines[3].op, ResourceChangeOp::Del);
        let epoch = ResourceChange::get_from_index(0, 0).await?.1;
        assert!(publish_seed(&author, &temp, 0).await.is_err());
        assert_eq!(ResourceChange::get_from_index(0, 0).await?.1, epoch);
        Ok(())
    }
}
