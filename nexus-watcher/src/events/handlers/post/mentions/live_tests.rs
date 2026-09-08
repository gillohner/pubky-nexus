use super::*;
use nexus_common::{
    db::RedisOps,
    models::{
        post::{PostDetails, PostInput},
        traits::Collection,
        user::{UserCounts, UserDetails, UserIngestor},
    },
    types::{DynError, Pagination},
    StackConfig, StackManager,
};
use pubky_app_specs::{traits::TimestampId, PubkyAppPost};
use serde_json::json;

async fn author() -> Result<PubkyId, DynError> {
    let id: PubkyId = pubky::Keypair::random().into();
    let details = UserDetails::from_pubky(id.clone());
    details.put_to_graph().await?;
    UserDetails::put_to_index(&[id.as_ref()], vec![Some(details)]).await?;
    UserCounts::default().put_to_index(&id).await?;
    Ok(id)
}

async fn put(author: &PubkyId, id: &str, kind: PostKind, content: String) -> Result<(), DynError> {
    let post = PostInput {
        kind,
        content,
        parent: None,
        embed: None,
        attachments: None,
        lock: None,
    };
    super::super::sync_put(post, author.clone(), id.into(), &UserIngestor::new([])).await?;
    Ok(())
}

async fn assert_mentions(
    author: &PubkyId,
    post_id: &str,
    expected: &[&PubkyId],
) -> Result<(), DynError> {
    let graph = PostRelationships::get_from_graph(author, post_id)
        .await?
        .expect("post in graph");
    let cache = PostRelationships::get_from_index(author, post_id)
        .await?
        .expect("post in cache");
    let expected: Vec<String> = expected.iter().map(|id| id.to_string()).collect();
    assert_eq!(
        graph
            .mentioned
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        cache
            .mentioned
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        expected
    );
    Ok(())
}

async fn history(user: &PubkyId) -> Result<Vec<(String, i64)>, DynError> {
    let notifications = Notification::get_by_id(
        user,
        Pagination {
            limit: Some(100),
            ..Default::default()
        },
    )
    .await?;
    let mut result = notifications
        .into_iter()
        .map(|notification| {
            serde_json::to_string(&notification.body).map(|body| (body, notification.timestamp))
        })
        .collect::<Result<Vec<_>, _>>()?;
    result.sort();
    Ok(result)
}

/// Run only against explicitly supplied disposable services. This test adds
/// random fixtures, never clears graph state or rotates the shared epoch.
#[tokio::test]
#[ignore = "requires EVENTKY_TEST_NEO4J_URI and EVENTKY_TEST_REDIS_URI"]
async fn current_kind_mentions_reconcile_and_retry_without_timestamp_changes(
) -> Result<(), DynError> {
    let files = tempfile::tempdir()?;
    let mut config = StackConfig::default();
    config.files_path = files.path().to_path_buf();
    config.db.neo4j.uri = std::env::var("EVENTKY_TEST_NEO4J_URI")?;
    config.db.redis = std::env::var("EVENTKY_TEST_REDIS_URI")?;
    StackManager::setup(&config).await?;
    let author = author().await?;
    let first = self::author().await?;
    let second = self::author().await?;
    let post_id = PubkyAppPost::default().create_id();
    let custom: PostKind = "event".parse().expect("valid kind");
    let social = |recipient: &PubkyId| {
        json!({
            "organizer": format!("pubky{second}"),
            "social": {"version":1,"text":format!("pubky{recipient} pk:{recipient}")}
        })
        .to_string()
    };

    put(&author, &post_id, custom.clone(), social(&first)).await?;
    assert_mentions(&author, &post_id, &[&first]).await?;
    let initial_history = history(&first).await?;
    assert_eq!(initial_history.len(), 1);
    assert!(history(&second).await?.is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    put(&author, &post_id, custom.clone(), social(&first)).await?;
    assert_eq!(history(&first).await?, initial_history);

    put(&author, &post_id, custom.clone(), social(&second)).await?;
    assert_mentions(&author, &post_id, &[&second]).await?;
    let second_history = history(&second).await?;
    assert_eq!(second_history.len(), 1);
    put(&author, &post_id, custom.clone(), social(&first)).await?;
    assert_mentions(&author, &post_id, &[&first]).await?;
    assert_eq!(history(&first).await?, initial_history);

    put(&author, &post_id, PostKind::Short, format!("pubky{second}")).await?;
    assert_mentions(&author, &post_id, &[&second]).await?;
    let native_history = history(&second).await?;
    // A kind transition may retain a distinct historical body, as native
    // notification identity includes kind. Retries cannot add or move it.
    assert_eq!(native_history.len(), 2);
    put(&author, &post_id, PostKind::Short, format!("pubky{second}")).await?;
    assert_eq!(history(&second).await?, native_history);

    put(&author, &post_id, PostKind::Short, format!("pk:{first}")).await?;
    assert_mentions(&author, &post_id, &[&first]).await?;
    let restored_history = history(&first).await?;
    PostDetails::remove_from_index_multiple_json(&[&[author.as_ref(), &post_id]]).await?;
    PostRelationships::remove_from_index_multiple_json(&[&[author.as_ref(), &post_id]]).await?;
    put(&author, &post_id, PostKind::Short, format!("pk:{first}")).await?;
    assert_mentions(&author, &post_id, &[&first]).await?;
    assert_eq!(history(&first).await?, restored_history);

    put(&author, &post_id, custom.clone(), social(&first)).await?;
    put(&author, &post_id, PostKind::Short, "[DELETED]".into()).await?;
    assert_mentions(&author, &post_id, &[]).await?;
    put(
        &author,
        &post_id,
        custom,
        json!({"social":{"version":2,"text":format!("pubky{first}")}}).to_string(),
    )
    .await?;
    assert_mentions(&author, &post_id, &[]).await?;
    Ok(())
}
