use crate::utils::host_url;
use anyhow::Result;
use deadpool_redis::redis::AsyncCommands;
use nexus_common::db::get_redis_conn;
use nexus_common::models::app_post::AppPostUri;
use nexus_common::models::event::resource_changes::{
    ResourceChange, ResourceChangeOp, RESOURCE_CHANGES_KEY,
};
use serde_json::json;

// Shared-list replacement requires nextest exclusivity.
#[tokio_shared_rt::test(shared)]
async fn post_and_tag_references_share_cursor_epoch_and_schema() -> Result<()> {
    let url = host_url().await;
    let client = httpc_test::new_client("")?;
    let mut conn = get_redis_conn().await?;
    let _: () = conn
        .del(&[RESOURCE_CHANGES_KEY, "Event:ResourceChanges:Epoch"])
        .await?;
    assert_eq!(
        client
            .do_get(&format!("{url}/v0/app-posts/changes"))
            .await?
            .status(),
        404
    );
    let author = "emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy";
    let parsed = AppPostUri::parse(&format!(
        "pubky://{author}/pub/example/v1/posts/0032SSN7Q4EVG/0032SSN7Q4EVJ-note.json"
    ))
    .unwrap();
    let post = ResourceChange::post(&parsed, ResourceChangeOp::Put)?;
    let tag = ResourceChange::tag(
        ResourceChangeOp::Put,
        post.resource_id.clone(),
        post.uri.clone(),
        author.into(),
        "note".into(),
        &format!("pubky://{author}/pub/example/tags/one"),
        post.at,
    )?;
    tag.store().await?;
    let endpoint = format!("{url}/v0/resources/changes");
    let first = client.do_get(&format!("{endpoint}?limit=1")).await?;
    let epoch = first.header("x-events-epoch");
    assert_eq!(first.json_body()?["lines"][0], serde_json::to_value(&tag)?);
    post.store().await?;
    ResourceChange::post(&parsed, ResourceChangeOp::Del)?
        .prepare_delete()
        .await?;
    ResourceChange::finish_delete(author, &parsed.version_uri()).await?;
    let page = client
        .do_get(&format!("{endpoint}?cursor=1&limit=2"))
        .await?;
    assert_eq!(page.header("x-events-epoch"), epoch);
    assert_eq!(page.header("x-events-length").as_deref(), Some("3"));
    let body = page.json_body()?;
    assert_eq!(body["next_cursor"], 3);
    assert_eq!(
        body["lines"][0],
        json!({
            "op":"put", "kind":"post", "author":author, "uri":parsed.uri(),
            "resource_id":post.resource_id, "object_path":parsed.version_path(), "at":post.at
        })
    );
    assert_eq!(body["lines"][1]["op"], "del");
    assert_eq!(body["lines"][1]["object_path"], parsed.version_path());
    assert_eq!(body["lines"][1]["uri"], parsed.uri());
    assert_eq!(body["lines"][1].as_object().unwrap().len(), 7);
    assert!(!conn.exists::<_, bool>("Event:AppPostChanges").await?);
    Ok(())
}
