//! Selected social posts remain social graph nodes; only their reference is duplicated.
use crate::errors::EventProcessorError;
use nexus_common::db::{
    fetch_row_from_graph, get_neo4j_graph,
    graph::{GraphError, Query},
};
use nexus_common::models::app_post::AppPostUri;
use nexus_common::models::event::resource_changes::{ResourceChange, ResourceChangeOp};

/// Called after the social handler succeeds. Graph metadata lets reseed recover a
/// selected head after Redis loss or a crash before append. No content is copied.
pub async fn after_put(path: &AppPostUri, kind: Option<String>) -> Result<(), EventProcessorError> {
    let line = kind
        .as_ref()
        .map(|_| ResourceChange::post(path, ResourceChangeOp::Put))
        .transpose()?;
    get_neo4j_graph()?
        .run(
            Query::new(
                "social_post_feed_head",
                r"
        MATCH (p:Post {author: $author, id: $id})
        SET p.post_feed_version = $version, p.post_feed_kind = $kind, p.post_feed_at = $at
    ",
            )
            .param("author", path.author.to_string())
            .param("id", path.id.clone())
            .param(
                "version",
                line.as_ref().map(|line| line.object_path.clone()),
            )
            .param("kind", kind)
            .param("at", line.as_ref().map(|line| line.at)),
        )
        .await
        .map_err(GraphError::from)?;
    if let Some(line) = line {
        line.store().await?;
    }
    Ok(())
}

pub async fn before_del(path: &AppPostUri) -> Result<(), EventProcessorError> {
    ResourceChange::prepare_version_delete(path).await?;
    // A graph-only successful PUT may not have populated Redis metadata yet.
    if fetch_row_from_graph(
        Query::new(
            "social_post_feed_delete",
            r"
        MATCH (p:Post {author: $author, id: $id})
        WHERE p.post_feed_version = $version
        RETURN p.post_feed_kind AS kind
    ",
        )
        .param("author", path.author.to_string())
        .param("id", path.id.clone())
        .param("version", path.version_path()),
    )
    .await?
    .is_some()
    {
        ResourceChange::post(path, ResourceChangeOp::Del)?
            .prepare_delete()
            .await?;
    }
    Ok(())
}

pub async fn after_del(path: &AppPostUri) -> Result<(), EventProcessorError> {
    // N3 may keep a tombstone when replies exist; it is no longer a seedable head.
    get_neo4j_graph()?
        .run(
            Query::new(
                "social_post_feed_clear",
                r"
        MATCH (p:Post {author: $author, id: $id})
        REMOVE p.post_feed_version, p.post_feed_kind, p.post_feed_at
    ",
            )
            .param("author", path.author.to_string())
            .param("id", path.id.clone()),
        )
        .await
        .map_err(GraphError::from)?;
    ResourceChange::finish_delete(path.author.as_ref(), &path.version_uri()).await?;
    Ok(())
}
