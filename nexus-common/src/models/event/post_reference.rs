//! Fork-only self references for app post versions.
//! The list, epoch, pending-delete journal and publication rules belong to the
//! resource feed. Only per-version recovery metadata is specific to posts.

use super::resource_changes::{
    write_seed_batch, ResourceChange, ResourceChangeKind, ResourceChangeOp, BATCH_SIZE,
};
use crate::db::graph::{GraphError, Query};
use crate::db::kv::{RedisError, RedisResult};
use crate::db::{get_neo4j_graph, get_redis_conn};
use crate::models::app_post::{valid_namespace, AppPostUri};
use crate::models::error::{ModelError, ModelResult};
use crate::universal_tag::normalize::{normalize_uri, resource_id};
use deadpool_redis::redis::{self, AsyncCommands};
use futures::TryStreamExt;

pub(super) const POST_VERSIONS_KEY: &str = "Event:ResourceChanges:PostVersions";

impl ResourceChange {
    /// The object names itself. The logical URI is normalized and hashed exactly
    /// like any Resource; the replay key still names the actual version file.
    pub fn post(path: &AppPostUri, op: ResourceChangeOp) -> ModelResult<Self> {
        if path.version.is_none() || !valid_namespace(&path.namespace) {
            return Err(ModelError::from_generic(
                "A post reference requires a stored app-namespace version",
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

    /// Every accepted version survives here until its DEL, including earlier
    /// edits after N3 removes the logical graph metadata.
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
}

pub(super) async fn seed_post_references(temp: &str) -> ModelResult<()> {
    let mut conn = get_redis_conn().await?;
    // Recover a graph-only commit or current heads after Redis loss. Historical
    // versions come from metadata; live appends follow the snapshot.
    let mut rows = get_neo4j_graph()?
        .execute(Query::new(
            "post_references_seed",
            r"
        MATCH (p:Resource) WHERE p.namespace IS NOT NULL AND p.version_uri IS NOT NULL
        RETURN p.version_uri AS version_uri, p.app_indexed_at AS at ORDER BY at, version_uri
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
            let Some(path) = AppPostUri::parse(&version_uri).filter(|path| path.version.is_some())
            else {
                continue;
            };
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
        // Older deployments may have stored social references in the same hash.
        // Only accepted app-version keys belong in a newly published snapshot.
        let lines: Vec<String> = records
            .into_iter()
            .filter(|(uri, _)| AppPostUri::parse(uri).is_some_and(|path| path.version.is_some()))
            .map(|(_, raw)| raw)
            .collect();
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
        let graph_head = path(&author, "other-example", "0032SSN7Q4EVG");
        let input = graph_head
            .read(br#"{"kind":"note","content":"opaque"}"#)
            .unwrap();
        assert!(AppPost::new(&graph_head, input).put().await?); // graph commit, no append yet
        let first = path(&author, "example", "0032SSN7Q4EVJ");
        let mut second = first.clone();
        second.version = Some("0032SSN7Q4EVM".into());
        second.label = Some("revised".into());
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
        assert_eq!(own[0].uri, graph_head.uri());
        assert_eq!(own[0].object_path, graph_head.version_path());
        // A version deletion preserves the other version's metadata, including its own kind.
        ResourceChange::post(&first, ResourceChangeOp::Del)?
            .prepare_delete()
            .await?;
        ResourceChange::finish_delete(first.author.as_ref(), &first.version_uri()).await?;
        ResourceChange::finish_delete(first.author.as_ref(), &first.version_uri()).await?;
        let (page, _, length) = ResourceChange::get_from_index(seed_length, 10).await?;
        assert_eq!(length, seed_length + 1);
        assert_eq!(page.lines.len(), 1);
        assert_eq!(page.lines[0].object_path, first.version_path());
        assert!(ResourceChange::version_record(&first).await?.is_none());
        assert!(ResourceChange::version_record(&second).await?.is_some());
        // A second file DEL names that file only. No synthetic whole-post deletion.
        ResourceChange::post(&second, ResourceChangeOp::Del)?
            .prepare_delete()
            .await?;
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
        AppPost::delete(&graph_head.uri()).await?;
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
    async fn live_seed_ignores_obsolete_social_graph_and_version_metadata(
    ) -> Result<(), crate::types::DynError> {
        StackManager::setup(&StackConfig::default()).await?;
        let author = pubky::Keypair::random().public_key().to_z32();
        let social = AppPostUri::parse_social(&format!(
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
                .param("id", social.id.clone())
                .param("uri", social.uri())
                .param("version", social.version_path()),
            )
            .await?;
        assert!(ResourceChange::post(&social, ResourceChangeOp::Put).is_err());
        let old = ResourceChange {
            op: ResourceChangeOp::Put,
            resource_id: resource_id(&social.uri()),
            uri: social.uri(),
            kind: ResourceChangeKind::Post,
            author: author.clone(),
            label: None,
            object_path: social.version_path(),
            at: 1234,
        };
        let mut conn = get_redis_conn().await?;
        let _: () = conn
            .hset(POST_VERSIONS_KEY, social.version_uri(), old.json()?)
            .await?;
        let app = path(&author, "example", "0032SSN7Q4EVM");
        let app_line = ResourceChange::post(&app, ResourceChangeOp::Put)?;
        app_line.store().await?;
        ResourceChange::reseed().await?;
        let lines = ResourceChange::get_from_index(0, 1000).await?.0.lines;
        let own: Vec<_> = lines
            .into_iter()
            .filter(|line| line.author == author)
            .collect();
        assert_eq!(own, [app_line]);
        let _: () = conn
            .hdel(
                POST_VERSIONS_KEY,
                &[social.version_uri(), app.version_uri()],
            )
            .await?;
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
        let version = path(&author, "example", "0032SSN7Q4EVG");
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
        ResourceChange::post(&version, ResourceChangeOp::Del)?
            .prepare_delete()
            .await?;
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
