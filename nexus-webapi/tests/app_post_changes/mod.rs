use crate::utils::{host_url, invalid_get_request};
use anyhow::Result;
use axum::http::StatusCode;
use deadpool_redis::redis::AsyncCommands;
use nexus_common::db::get_redis_conn;
use nexus_common::models::app_post::AppPostUri;
use nexus_common::models::event::app_post_changes::{
    AppPostChange, AppPostChangeOp, APP_POST_CHANGES_KEY,
};
use serde_json::json;

// Exclusive in nextest because this test replaces the feed list/epoch.
#[tokio_shared_rt::test(shared)]
async fn pages_headers_bounds_and_read_only_empty_feed() -> Result<()> {
    let url = host_url().await;
    let client = httpc_test::new_client("")?;
    let mut conn = get_redis_conn().await?;
    let epoch_key = format!("{APP_POST_CHANGES_KEY}:Epoch");
    let _: () = conn
        .del(&[APP_POST_CHANGES_KEY, epoch_key.as_str()])
        .await?;
    let path = format!("{url}/v0/app-posts/changes");
    let empty = client.do_get(&path).await?;
    assert_eq!(empty.status(), 200);
    assert_eq!(empty.json_body()?, json!({"lines": [], "next_cursor": 0}));
    assert_eq!(empty.header("x-events-length").as_deref(), Some("0"));
    assert_eq!(empty.header("x-events-epoch"), None);
    assert!(!conn.exists::<_, bool>(&epoch_key).await?);
    assert!(!conn.exists::<_, bool>(APP_POST_CHANGES_KEY).await?);

    let author = "emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy";
    let version_path = "/pub/maps/v1/posts/0032SSN7Q4EVG/0032SSN7Q4EVG.json";
    let parsed = AppPostUri::parse(&format!("pubky://{author}{version_path}")).unwrap();
    let line = AppPostChange::new(&parsed, "route".into(), AppPostChangeOp::Put);
    let resource_before: Vec<String> = conn.lrange("Event:ResourceChanges", 0, -1).await?;
    line.store().await?;
    AppPostChange::prepare_version_delete(&parsed).await?;
    AppPostChange::finish_delete(&parsed).await?;
    assert_eq!(
        conn.lrange::<_, Vec<String>>("Event:ResourceChanges", 0, -1)
            .await?,
        resource_before
    );
    let first = client.do_get(&format!("{path}?limit=1")).await?;
    assert_eq!(first.status(), 200);
    assert_eq!(first.header("x-events-length").as_deref(), Some("2"));
    let epoch = first.header("x-events-epoch").expect("initialized epoch");
    assert_eq!(first.json_body()?["next_cursor"], 1);
    assert_eq!(
        first.json_body()?["lines"][0],
        json!({
            "op":"put", "author":author, "post":parsed.path(), "version":version_path,
            "namespace":"maps", "post_kind":"route", "at":line.at
        })
    );
    assert_eq!(first.json_body()?["lines"][0]["op"], "put");
    assert!(first
        .header("access-control-expose-headers")
        .unwrap()
        .contains("x-events-epoch"));
    let second = client.do_get(&format!("{path}?cursor=1&limit=1")).await?;
    assert_eq!(second.json_body()?["lines"][0]["op"], "del");
    assert_eq!(second.json_body()?["next_cursor"], 2);
    assert_eq!(second.json_body()?["lines"][0]["version"], version_path);
    assert_eq!(
        second.header("x-events-epoch").as_deref(),
        Some(epoch.as_str())
    );
    for cursor in [2, u64::MAX] {
        let response = client
            .do_get(&format!("{path}?cursor={cursor}&limit=1000"))
            .await?;
        assert_eq!(
            response.json_body()?,
            json!({"lines": [], "next_cursor": cursor})
        );
    }
    for query in [
        "limit=0",
        "limit=1001",
        "cursor=-1",
        "cursor=18446744073709551616",
    ] {
        invalid_get_request(
            &format!("/v0/app-posts/changes?{query}"),
            StatusCode::BAD_REQUEST,
        )
        .await?;
    }
    AppPostChange::reseed().await?;
    let rebuilt = client.do_get(&path).await?;
    assert_ne!(
        rebuilt.header("x-events-epoch").as_deref(),
        Some(epoch.as_str())
    );
    assert_eq!(
        conn.lrange::<_, Vec<String>>("Event:ResourceChanges", 0, -1)
            .await?,
        resource_before
    );
    Ok(())
}
