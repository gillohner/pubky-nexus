//! Integration tests for the plugin dispatch system.
//!
//! Uses a minimal TestPlugin that writes/deletes Neo4j nodes to verify
//! that the event dispatcher correctly routes homeserver events to plugins.

use anyhow::Result;
use async_trait::async_trait;
use axum::Router;
use futures::TryStreamExt;
use nexus_common::db::get_neo4j_graph;
use nexus_common::db::graph::Query;
use nexus_common::plugin::{PluginContext, PluginManifest};
use nexus_common::types::DynError;
use nexus_watcher::testing::WatcherTest;
use pubky::Keypair;
use pubky_app_specs::PubkyAppUser;
use std::sync::Arc;

// ── Minimal test plugin ─────────────────────────────────────────────────────

/// A plugin that claims `/pub/testplugin.app/` and creates `:TestPluginItem`
/// nodes in Neo4j. Used solely for integration-testing the dispatch pipeline.
struct TestPlugin;

#[async_trait]
impl nexus_common::plugin::NexusPlugin for TestPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            name: "testplugin",
            namespace: "/pub/testplugin.app/",
        }
    }

    async fn handle_put(
        &self,
        uri: &str,
        data: &[u8],
        user_id: &str,
        _ctx: &PluginContext,
    ) -> Result<(), DynError> {
        // Extract the item ID from the URI: .../items/{id}
        let item_id = uri.rsplit('/').next().ok_or("missing item id in URI")?;

        // Deserialize the blob
        let payload: serde_json::Value = serde_json::from_slice(data)?;
        let data_str = payload.get("data").and_then(|v| v.as_str()).unwrap_or("");

        let compound_id = format!("{user_id}:{item_id}");

        let graph = get_neo4j_graph()?;
        graph
            .run(
                Query::new(
                    "test_plugin_put",
                    "MERGE (n:TestPluginItem {id: $id})
                     SET n.data = $data, n.user_id = $user_id",
                )
                .param("id", compound_id.as_str())
                .param("data", data_str)
                .param("user_id", user_id),
            )
            .await?;

        Ok(())
    }

    async fn handle_del(
        &self,
        uri: &str,
        user_id: &str,
        _ctx: &PluginContext,
    ) -> Result<(), DynError> {
        let item_id = uri.rsplit('/').next().ok_or("missing item id in URI")?;

        let compound_id = format!("{user_id}:{item_id}");

        let graph = get_neo4j_graph()?;
        graph
            .run(
                Query::new(
                    "test_plugin_del",
                    "MATCH (n:TestPluginItem {id: $id}) DETACH DELETE n",
                )
                .param("id", compound_id.as_str()),
            )
            .await?;

        Ok(())
    }

    fn routes(&self, _ctx: PluginContext) -> Router {
        Router::new()
    }

    async fn setup_schema(&self, _ctx: &PluginContext) -> Result<(), DynError> {
        let graph = get_neo4j_graph()?;
        graph
            .run(Query::new(
                "test_plugin_schema",
                "CREATE CONSTRAINT test_plugin_item_id IF NOT EXISTS
                 FOR (n:TestPluginItem) REQUIRE n.id IS UNIQUE",
            ))
            .await?;
        Ok(())
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn count_test_plugin_items(compound_id: &str) -> Result<i64> {
    let graph = get_neo4j_graph()?;
    let mut stream = graph
        .execute(
            Query::new(
                "test_count_plugin_items",
                "MATCH (n:TestPluginItem {id: $id}) RETURN count(n) AS cnt",
            )
            .param("id", compound_id),
        )
        .await?;
    let row = stream
        .try_next()
        .await?
        .expect("count query must return a row");
    Ok(row.get("cnt")?)
}

async fn get_test_plugin_item_data(compound_id: &str) -> Result<Option<String>> {
    let graph = get_neo4j_graph()?;
    let mut stream = graph
        .execute(
            Query::new(
                "test_get_plugin_item",
                "MATCH (n:TestPluginItem {id: $id}) RETURN n.data AS data",
            )
            .param("id", compound_id),
        )
        .await?;
    match stream.try_next().await? {
        Some(row) => Ok(Some(row.get("data")?)),
        None => Ok(None),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

/// 1b. Plugin PUT event creates a Neo4j node.
#[tokio_shared_rt::test(shared)]
async fn test_plugin_put_creates_node() -> Result<()> {
    let mut test = WatcherTest::setup_with_plugins(vec![Arc::new(TestPlugin)]).await?;

    let user_kp = Keypair::random();
    let user = PubkyAppUser {
        name: "PluginTestUser".to_string(),
        bio: None,
        image: None,
        links: None,
        status: None,
    };
    let user_id = test.create_user(&user_kp, &user).await?;

    // Write a JSON blob under the plugin namespace.
    let item_path: pubky::ResourcePath = "/pub/testplugin.app/items/ITEM001".parse()?;
    let payload = serde_json::json!({ "data": "hello-plugin" });
    test.put(&user_kp, &item_path, &payload).await?;

    // Verify the plugin created the node.
    let compound_id = format!("{user_id}:ITEM001");
    let data = get_test_plugin_item_data(&compound_id).await?;
    assert_eq!(data.as_deref(), Some("hello-plugin"));

    // Cleanup
    test.del(&user_kp, &item_path).await?;
    test.cleanup_user(&user_kp).await?;
    Ok(())
}

/// 1c. Plugin DEL event removes the Neo4j node.
#[tokio_shared_rt::test(shared)]
async fn test_plugin_del_removes_node() -> Result<()> {
    let mut test = WatcherTest::setup_with_plugins(vec![Arc::new(TestPlugin)]).await?;

    let user_kp = Keypair::random();
    let user = PubkyAppUser {
        name: "PluginDelUser".to_string(),
        bio: None,
        image: None,
        links: None,
        status: None,
    };
    let user_id = test.create_user(&user_kp, &user).await?;

    // Create then delete.
    let item_path: pubky::ResourcePath = "/pub/testplugin.app/items/ITEM002".parse()?;
    let payload = serde_json::json!({ "data": "to-be-deleted" });
    test.put(&user_kp, &item_path, &payload).await?;

    let compound_id = format!("{user_id}:ITEM002");
    assert_eq!(count_test_plugin_items(&compound_id).await?, 1);

    test.del(&user_kp, &item_path).await?;
    assert_eq!(count_test_plugin_items(&compound_id).await?, 0);

    test.cleanup_user(&user_kp).await?;
    Ok(())
}

/// 1d. Non-plugin event falls through to the social handler.
#[tokio_shared_rt::test(shared)]
async fn test_non_plugin_event_handled_by_social() -> Result<()> {
    let mut test = WatcherTest::setup_with_plugins(vec![Arc::new(TestPlugin)]).await?;

    let user_kp = Keypair::random();
    let user = PubkyAppUser {
        name: "SocialFallthrough".to_string(),
        bio: None,
        image: None,
        links: None,
        status: None,
    };
    let user_id = test.create_user(&user_kp, &user).await?;

    // Write a standard pubky-app post (NOT under testplugin namespace).
    let post = pubky_app_specs::PubkyAppPost {
        content: "social handler should index this".to_string(),
        kind: pubky_app_specs::PubkyAppPostKind::Short,
        parent: None,
        embed: None,
        attachments: None,
    };
    let (post_id, post_path) = test.create_post(&user_kp, &post).await?;

    // Social handler should have indexed the Post node.
    let graph = get_neo4j_graph()?;
    let mut stream = graph
        .execute(
            Query::new(
                "test_social_post_exists",
                "MATCH (u:User {id: $uid})-[:AUTHORED]->(p:Post {id: $pid})
                 RETURN count(p) AS cnt",
            )
            .param("uid", user_id.as_str())
            .param("pid", post_id.as_str()),
        )
        .await?;
    let cnt: i64 = stream.try_next().await?.unwrap().get("cnt")?;
    assert_eq!(cnt, 1, "Social handler should have created the Post node");

    // No TestPluginItem should exist.
    let plugin_cnt: i64 = {
        let mut s = graph
            .execute(
                Query::new(
                    "test_no_plugin_items",
                    "MATCH (n:TestPluginItem) WHERE n.user_id = $uid RETURN count(n) AS cnt",
                )
                .param("uid", user_id.as_str()),
            )
            .await?;
        s.try_next().await?.unwrap().get("cnt")?
    };
    assert_eq!(plugin_cnt, 0, "Plugin should not have handled this event");

    test.cleanup_post(&user_kp, &post_path).await?;
    test.cleanup_user(&user_kp).await?;
    Ok(())
}

/// 1e. Unknown namespace not claimed by any plugin falls through gracefully.
#[tokio_shared_rt::test(shared)]
async fn test_unknown_namespace_falls_through() -> Result<()> {
    let mut test = WatcherTest::setup_with_plugins(vec![Arc::new(TestPlugin)]).await?;

    let user_kp = Keypair::random();
    let user = PubkyAppUser {
        name: "UnknownNsUser".to_string(),
        bio: None,
        image: None,
        links: None,
        status: None,
    };
    let user_id = test.create_user(&user_kp, &user).await?;

    // Write under a namespace no plugin claims.
    // The social handler will also fail to parse it (unknown resource type),
    // but it should not crash — the event is skipped gracefully.
    let unknown_path: pubky::ResourcePath = "/pub/otherapp.app/items/X".parse()?;
    let payload = serde_json::json!({ "data": "orphan" });
    test.put(&user_kp, &unknown_path, &payload).await?;

    // No TestPluginItem should have been created.
    let graph = get_neo4j_graph()?;
    let mut stream = graph
        .execute(
            Query::new(
                "test_no_orphan_items",
                "MATCH (n:TestPluginItem) WHERE n.user_id = $uid RETURN count(n) AS cnt",
            )
            .param("uid", user_id.as_str()),
        )
        .await?;
    let cnt: i64 = stream.try_next().await?.unwrap().get("cnt")?;
    assert_eq!(cnt, 0);

    test.cleanup_user(&user_kp).await?;
    Ok(())
}
