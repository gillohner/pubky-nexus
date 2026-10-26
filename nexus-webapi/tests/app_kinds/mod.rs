use crate::utils::{get_request, host_url, invalid_get_request};
use anyhow::Result;
use axum::http::StatusCode;
use nexus_common::db::{get_neo4j_graph, graph::Query};
use nexus_common::models::app_post::{AppPost, AppPostUri};
use serde_json::json;

#[tokio_shared_rt::test(shared)]
async fn namespace_stream_is_explicit_filtered_and_content_free() -> Result<()> {
    host_url().await;
    let author = pubky::Keypair::random().public_key().to_z32();
    let graph = get_neo4j_graph()?;
    graph
        .run(
            Query::new("test_app_stream_author", "CREATE (:User {id: $author})")
                .param("author", author.clone()),
        )
        .await?;
    let namespace = "music";
    let mut uris = vec![];
    for (index, (id, kind)) in [
        ("0032SSN7Q4EVG", "session"),
        ("0032SSN7Q4EVJ", "listen"),
        ("0032SSN7Q4EVM", "listen"),
    ]
    .into_iter()
    .enumerate()
    {
        let uri = format!("pubky://{author}/pub/{namespace}/v1/posts/{id}");
        let path = AppPostUri::parse(&format!("{uri}/{id}.json")).unwrap();
        let input = path
            .read(&serde_json::to_vec(
                &json!({"kind":kind, "content":"unindexed content", "ext":{"unindexed": true}}),
            )?)
            .unwrap();
        let mut post = AppPost::new(&path, input);
        post.indexed_at = ((index + 1) * 1000) as i64;
        assert!(post.put().await?);
        uris.push(uri);
    }
    for query in [
        String::new(),
        format!("?source=author&author_id={author}"),
        "?sorting=total_engagement".into(),
        "?namespace=pubky.app".into(),
    ] {
        let posts = get_request(&format!("/v0/stream/posts{query}")).await?;
        assert!(posts
            .as_array()
            .unwrap()
            .iter()
            .all(|post| post.get("namespace").is_none() && post["details"]["author"] != author));
    }
    let endpoint = format!("/v0/stream/posts?namespace={namespace}&author_id={author}");
    let posts = get_request(&endpoint).await?;
    assert_eq!(posts.as_array().unwrap().len(), 3);
    assert!(posts
        .as_array()
        .unwrap()
        .iter()
        .all(|post| post["namespace"] == namespace
            && post.get("content").is_none()
            && post.get("ext").is_none()));
    let listens = get_request(&format!("{endpoint}&kind=listen")).await?;
    assert_eq!(listens.as_array().unwrap().len(), 2);
    let sessions = get_request(&format!("{endpoint}&exclude_kinds=listen&source=author")).await?;
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    assert_eq!(sessions[0]["kind"], "session");
    let page = get_request(&format!("{endpoint}&skip=1&limit=1")).await?;
    assert_eq!(page.as_array().unwrap().len(), 1);
    assert_eq!(page[0], posts[1]);
    let ascending = get_request(&format!("{endpoint}&order=ascending&start=2000&end=3000")).await?;
    assert_eq!(ascending.as_array().unwrap().len(), 2);
    assert_eq!(ascending[0]["indexed_at"], 2000);
    assert_eq!(ascending[1]["indexed_at"], 3000);
    let bounded = get_request(&format!("{endpoint}&start=2500&end=1000")).await?;
    assert_eq!(bounded.as_array().unwrap().len(), 2);
    assert_eq!(bounded[0]["indexed_at"], 2000);
    assert!(posts[0]["version_uri"].as_str().unwrap().ends_with(".json"));
    assert_eq!(
        get_request(&format!("{endpoint}&kind=listening")).await?,
        json!([])
    );
    for suffix in [
        "&kind=listen&exclude_kinds=session",
        "&kind=UPPER",
        "&source=following",
        "&tags=hello",
        "&sorting=total_engagement",
        "&include_attachment_metadata=true",
        "&exclude_kinds=listen,,session",
    ] {
        invalid_get_request(&format!("{endpoint}{suffix}"), StatusCode::BAD_REQUEST).await?;
    }
    invalid_get_request(
        "/v0/stream/posts?namespace=../music",
        StatusCode::BAD_REQUEST,
    )
    .await?;
    invalid_get_request(
        &format!("/v0/stream/posts/keys?namespace={namespace}"),
        StatusCode::BAD_REQUEST,
    )
    .await?;
    for uri in uris {
        AppPost::delete(&uri).await?;
    }
    graph
        .run(
            Query::new(
                "test_app_stream_cleanup",
                "MATCH (u:User {id: $author}) DETACH DELETE u",
            )
            .param("author", author),
        )
        .await?;
    Ok(())
}
