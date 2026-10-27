use super::posts::utils::test_user;
use super::utils::watcher::{generate_post_id, retrieve_and_handle_event_line, WatcherTest};
use anyhow::Result;
use nexus_common::db::{fetch_row_from_graph, graph::Query};
use nexus_common::models::event::resource_changes::{ResourceChange, ResourceChangeOp};
use nexus_common::models::post::PostDetails;
use pubky::{Keypair, ResourcePath};
use pubky_social_specs::{
    post_uri_builder,
    traits::{HasIdPath, HashId},
    PubkySocialFollow, PubkySocialTag,
};
use serde_json::json;

#[tokio_shared_rt::test(shared)]
async fn indexes_v1_versions_follows_collections_and_resource_tags() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let key = Keypair::random();
    let author = test
        .create_user(&key, &test_user("Social v1 fork", ""))
        .await?;
    let other_key = Keypair::random();
    let other = test
        .create_user(&other_key, &test_user("Social v1 followee", ""))
        .await?;
    let id = generate_post_id();
    let edit = generate_post_id();
    let first: ResourcePath = format!("/pub/social/v1/posts/{id}/{id}.json").parse()?;
    let second: ResourcePath = format!("/pub/social/v1/posts/{id}/{edit}.json").parse()?;
    let uri = post_uri_builder(author.clone(), id.clone());
    test.put(
        &key,
        &first,
        &json!({"kind":"note", "content":"first", "attachments":[]}),
    )
    .await?;
    assert_eq!(
        PostDetails::get_by_id(&author, &id).await?.unwrap().uri,
        uri
    );
    test.put(
        &key,
        &second,
        &json!({"kind":"note", "content":"edited", "attachments":[]}),
    )
    .await?;
    assert_eq!(
        PostDetails::get_by_id(&author, &id).await?.unwrap().content,
        "edited"
    );

    let follow: ResourcePath = PubkySocialFollow::create_path(&other).parse()?;
    test.put(&key, &follow, &PubkySocialFollow::new()).await?;
    let row = fetch_row_from_graph(
        Query::new(
            "test_v1_follow",
            "MATCH (:User {id: $author})-[f:FOLLOWS]->(:User {id: $other}) RETURN count(f) AS n",
        )
        .param("author", author.clone())
        .param("other", other.clone()),
    )
    .await?
    .unwrap();
    assert_eq!(row.get::<i64>("n")?, 1);

    let collection_id = generate_post_id();
    let collection: ResourcePath =
        format!("/pub/social/v1/posts/{collection_id}/{collection_id}.json").parse()?;
    let content =
        json!({"name":"v1 collection", "items":[{"uri":uri}], "layout":"grid"}).to_string();
    test.put(
        &key,
        &collection,
        &json!({"kind":"collection", "content":content, "attachments":[]}),
    )
    .await?;
    let row = fetch_row_from_graph(Query::new("test_v1_collection", "MATCH (c:Post {id: $collection})-[:COLLECTED]->(p:Post {id: $post}) RETURN count(p) AS n").param("collection", collection_id.clone()).param("post", id.clone())).await?.unwrap();
    assert_eq!(row.get::<i64>("n")?, 1);

    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let mut tag_paths = vec![];
    for target in [
        format!("https://example.com/v1/{author}"),
        format!("pubky://{author}/pub/music/v1/posts/{id}"),
    ] {
        let tag = PubkySocialTag::new(target, "v1-test".into());
        let path: ResourcePath = PubkySocialTag::create_path(&tag.create_id()).parse()?;
        test.put(&key, &path, &tag).await?;
        tag_paths.push(path);
    }
    let (page, _, _) = ResourceChange::get_from_index(cursor, 1000).await?;
    let own: Vec<_> = page
        .lines
        .into_iter()
        .filter(|line| line.author == author)
        .collect();
    assert_eq!(own.len(), 2);
    assert!(own.iter().all(|line| line.op == ResourceChangeOp::Put
        && line.object_path.starts_with("/pub/social/v1/tags/")));
    for path in tag_paths {
        test.del(&key, &path).await?;
    }
    let (page, _, _) = ResourceChange::get_from_index(cursor, 1000).await?;
    assert_eq!(
        page.lines
            .iter()
            .filter(|line| line.author == author && line.op == ResourceChangeOp::Del)
            .count(),
        2
    );
    test.del(&key, &collection).await?;
    test.del(&key, &follow).await?;
    // N2 contract: deleting any version deletes the logical post; later DELs no-op.
    test.del(&key, &first).await?;
    assert!(PostDetails::get_by_id(&author, &id).await?.is_none());
    test.del(&key, &second).await?;
    retrieve_and_handle_event_line(
        &format!("DEL pubky://{author}{second}"),
        test.event_processor_runner.event_handler.clone(),
    )
    .await?;
    test.cleanup_user(&other_key).await?;
    test.cleanup_user(&key).await?;
    Ok(())
}
