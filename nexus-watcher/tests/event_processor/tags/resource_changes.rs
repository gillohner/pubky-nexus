use crate::event_processor::posts::utils::{short_post, test_user};
use crate::event_processor::utils::watcher::{retrieve_and_handle_event_line, WatcherTest};
use anyhow::Result;
use chrono::Utc;
use nexus_common::models::event::resource_changes::{
    ResourceChange, ResourceChangeKind, ResourceChangeOp,
};
use pubky::{Keypair, ResourcePath};
use pubky_app_specs::{post_uri_builder, traits::HashId, user_uri_builder, PubkyAppTag};

async fn authored_since(author: &str, mut cursor: u64) -> Result<Vec<ResourceChange>> {
    let mut lines = Vec::new();
    loop {
        let (page, _, _) = ResourceChange::get_from_index(cursor, 1000).await?;
        if page.lines.is_empty() {
            break;
        }
        cursor = page.next_cursor;
        lines.extend(page.lines.into_iter().filter(|line| line.author == author));
    }
    Ok(lines)
}

#[tokio_shared_rt::test(shared)]
async fn test_resource_changes_put_del_and_replay() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let key = Keypair::random();
    let author = test
        .create_user(&key, &test_user("Reference feed", "feed"))
        .await?;
    let (_, _, start) = ResourceChange::get_from_index(0, 0).await?;
    let handler = test.event_processor_runner.event_handler.clone();
    for target in [
        format!("HTTPS://Example.COM:443/feed/{author}#fragment"),
        format!("pubky://{author}/pub/another.app/events/one"),
    ] {
        // Use the specs constructor so the file ID covers the sanitized object.
        let tag = PubkyAppTag::new(target.clone(), "feed".into());
        let path = format!("/pub/test.app/tags/{}", tag.create_id());
        let storage: ResourcePath = path.parse()?;
        let tag_uri = format!("pubky://{author}{path}");
        let before = authored_since(&author, start).await?.len();
        test.put(&key, &storage, &tag).await?;
        retrieve_and_handle_event_line(&format!("PUT {tag_uri}"), handler.clone()).await?;
        let lines = authored_since(&author, start).await?;
        assert_eq!(
            lines.len(),
            before + 2,
            "documented at-least-once put delivery"
        );
        assert_eq!(
            lines[before],
            lines[before + 1],
            "replay retains the graph's indexed_at"
        );
        let put = &lines[before];
        assert_eq!(put.kind, ResourceChangeKind::Tag);
        assert_eq!(put.op, ResourceChangeOp::Put);
        assert_eq!(put.object_path, path);
        assert_eq!(put.label.as_deref(), Some("feed"));
        let (normalized, _) = pubky_resource_id::normalize_uri(&target).unwrap();
        assert_eq!(put.uri, normalized);
        assert_eq!(put.resource_id, pubky_resource_id::resource_id(&normalized));
        test.del(&key, &storage).await?;
        retrieve_and_handle_event_line(&format!("DEL {tag_uri}"), handler.clone()).await?;
        let lines = authored_since(&author, start).await?;
        assert_eq!(
            lines.len(),
            before + 3,
            "delete acknowledgement/replay is idempotent"
        );
        let deleted = lines.last().unwrap();
        assert_eq!(deleted.op, ResourceChangeOp::Del);
        assert_eq!(deleted.resource_id, put.resource_id);
        assert_eq!(deleted.object_path, put.object_path);
        assert!(deleted.at >= put.at);
    }
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn test_resource_changes_excludes_posts_and_users() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let key = Keypair::random();
    let author = test
        .create_user(&key, &test_user("Known references", "feed"))
        .await?;
    let (post_id, post_path) = test
        .create_post(&key, &short_post("Known reference"))
        .await?;
    let (_, _, start) = ResourceChange::get_from_index(0, 0).await?;
    for target in [
        post_uri_builder(author.clone(), post_id),
        user_uri_builder(author.clone()),
    ] {
        let tag = PubkyAppTag {
            uri: target,
            label: "known".into(),
            created_at: Utc::now().timestamp_millis(),
        };
        let path: ResourcePath = format!("/pub/test.app/tags/{}", tag.create_id()).parse()?;
        test.put(&key, &path, &tag).await?;
        test.del(&key, &path).await?;
    }
    assert!(authored_since(&author, start).await?.is_empty());
    test.cleanup_post(&key, &post_path).await?;
    test.cleanup_user(&key).await?;
    Ok(())
}
