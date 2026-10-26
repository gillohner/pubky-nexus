use crate::utils::{get_request, host_url, invalid_get_request, post_request};
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
    let namespace = "example";
    let mut uris = vec![];
    for (index, (id, kind)) in [
        ("0032SSN7Q4EVG", "article"),
        ("0032SSN7Q4EVJ", "note"),
        ("0032SSN7Q4EVM", "note"),
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
        "?namespace=social".into(),
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
    for post in posts.as_array().unwrap() {
        assert_eq!(
            post["id"],
            nexus_common::universal_tag::normalize::resource_id(post["uri"].as_str().unwrap())
        );
        assert!(post["uri"]
            .as_str()
            .unwrap()
            .ends_with(post["post_id"].as_str().unwrap()));
        let resource =
            post_request("/v0/stream/resources/by_ids", json!({"uris":[post["uri"]]})).await?;
        assert_eq!(resource[0]["details"]["id"], post["id"]);
        assert_eq!(resource[0]["details"]["uri"], post["uri"]);
    }
    let notes = get_request(&format!("{endpoint}&kind=note")).await?;
    assert_eq!(notes.as_array().unwrap().len(), 2);
    let articles = get_request(&format!("{endpoint}&exclude_kinds=note&source=author")).await?;
    assert_eq!(articles.as_array().unwrap().len(), 1);
    assert_eq!(articles[0]["kind"], "article");
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
        get_request(&format!("{endpoint}&kind=memo")).await?,
        json!([])
    );
    for suffix in [
        "&kind=note&exclude_kinds=article",
        "&kind=UPPER",
        "&source=following",
        "&tags=hello",
        "&sorting=total_engagement",
        "&include_attachment_metadata=true",
        "&exclude_kinds=note,,article",
    ] {
        invalid_get_request(&format!("{endpoint}{suffix}"), StatusCode::BAD_REQUEST).await?;
    }
    for namespace in [
        "",
        "../example",
        "example.app",
        "example_app",
        "Example",
        "1example",
        "-example",
        "ex%C3%A1mple",
        "abcdefghijklmnopqrstuvwxyz0123456",
    ] {
        invalid_get_request(
            &format!("/v0/stream/posts?namespace={namespace}"),
            StatusCode::BAD_REQUEST,
        )
        .await?;
    }
    for namespace in ["x", "example-2", "abcdefghijklmnopqrstuvwxyz012345"] {
        assert_eq!(
            get_request(&format!(
                "/v0/stream/posts?namespace={namespace}&author_id={author}"
            ))
            .await?,
            json!([])
        );
    }
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
