use super::posts::utils::{short_post, test_user};
use super::utils::watcher::{generate_post_id, retrieve_and_handle_event_line, WatcherTest};
use anyhow::Result;
use nexus_common::db::{fetch_row_from_graph, graph::Query};
use nexus_common::models::app_post::{AppPost, AppPostUri};
use nexus_common::models::event::resource_changes::{
    ResourceChange, ResourceChangeKind, ResourceChangeOp,
};
use nexus_common::models::post::{PostDetails, PostRelationships};
use nexus_watcher::errors::EventProcessorError;
use nexus_watcher::events::handlers::app_post;
use pubky::{Keypair, ResourcePath};
use pubky_social_specs::legacy_v0::post_uri_builder;
use serde_json::{json, Value};

fn envelope(_id: &str, kind: &str) -> Value {
    json!({"kind": kind, "content": "  {uninterpreted app data}  ", "attachments": [], "ext": {"anything": [null, true, 7]}})
}

async fn own_lines(author: &str, mut cursor: u64) -> Result<Vec<ResourceChange>> {
    let mut lines = Vec::new();
    loop {
        let (page, _, _) = ResourceChange::get_from_index(cursor, 1000).await?;
        if page.lines.is_empty() {
            break;
        }
        cursor = page.next_cursor;
        lines.extend(
            page.lines
                .into_iter()
                .filter(|line| line.author == author && line.kind == ResourceChangeKind::Post),
        );
    }
    Ok(lines)
}

async fn setup() -> Result<(WatcherTest, Keypair, String)> {
    let mut test = WatcherTest::setup(None).await?;
    let key = Keypair::random();
    let author = test
        .create_user(&key, &test_user("App kinds", "app-kinds"))
        .await?;
    Ok((test, key, author))
}

#[tokio_shared_rt::test(shared)]
async fn put_edges_edit_and_del() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/example/v1/posts/{id}/{id}.json").parse()?;
    let version_uri = format!("pubky://{author}{path}");
    let uri = AppPostUri::parse(&version_uri).unwrap().uri();
    let mut body = envelope(&id, "note");
    body["parent"] = json!(format!("pubky://{author}/pub/album/catalog.json"));
    body["embed"] = json!("https://example.com/album");
    test.put(&key, &path, &body).await?;
    let post = AppPost::get(&uri).await?.expect("indexed app post");
    assert_eq!(post.namespace, "example");
    assert_eq!(post.kind, "note");
    assert_eq!(post.post_id, id);
    assert_eq!(
        post.id,
        nexus_common::universal_tag::normalize::resource_id(&uri)
    );
    let row = fetch_row_from_graph(Query::new("test_app_edges", "MATCH (u:User)-[:AUTHORED]->(p:Resource {uri: $uri}) RETURN u.id AS author, p.content IS NULL AND p.ext IS NULL AS opaque, COUNT { (p)-[:APP_REPLIED]->(:Resource) } AS parents, COUNT { (p)-[:APP_EMBEDDED]->(:Resource) } AS embeds").param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<String>("author")?, author);
    assert!(row.get::<bool>("opaque")?);
    assert_eq!(row.get::<i64>("parents")?, 1);
    assert_eq!(row.get::<i64>("embeds")?, 1);
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 1);
    assert_eq!(
        serde_json::to_value(&lines[0])?,
        json!({"op":"put", "author":author, "uri":uri, "resource_id":post.id, "object_path": path.to_string(), "kind":"post", "at":lines[0].at})
    );

    body["parent"] = Value::Null;
    body["embed"] = Value::Null;
    body["kind"] = json!("article");
    let edit = generate_post_id();
    let edited_path: ResourcePath =
        format!("/pub/example/v1/posts/{id}/{edit}-note.json").parse()?;
    test.put(&key, &edited_path, &body).await?;
    assert_eq!(
        AppPost::get(&uri).await?.unwrap().version_uri,
        format!("pubky://{author}{edited_path}")
    );
    let row = fetch_row_from_graph(Query::new("test_app_edit", "MATCH (p:Resource {uri: $uri}) RETURN COUNT { (p)-[:APP_REPLIED|APP_EMBEDDED]->() } AS edges").param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<i64>("edges")?, 0);
    test.del(&key, &path).await?;
    assert!(AppPost::get(&uri).await?.is_none());
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2].op, ResourceChangeOp::Del);
    assert_eq!(lines[2].kind, ResourceChangeKind::Post);
    assert_eq!(lines[2].object_path.as_str(), path.to_string().as_str());
    // N3 removes app metadata on the first DEL, but every stored version is reported.
    test.del(&key, &edited_path).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[3].uri, uri);
    assert_eq!(
        lines[3].object_path.as_str(),
        edited_path.to_string().as_str()
    );
    test.cleanup_user(&key).await?;
    Ok(())
}

// Regression for the formerly special-cased music/listening payload: it now
// follows exactly the same graph and reference behavior as any app envelope.
#[tokio_shared_rt::test(shared)]
async fn every_accepted_app_kind_gets_resource_and_reference() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    for (namespace, kind) in [("example", "note"), ("music", "listening")] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/{namespace}/v1/posts/{id}/{id}.json").parse()?;
        let uri = format!("pubky://{author}/pub/{namespace}/v1/posts/{id}");
        let mut body = envelope(&id, kind);
        body["parent"] = json!("https://example.com/parent");
        body["embed"] = json!("https://example.com/embed");
        test.put(&key, &path, &body).await?;
        let post = AppPost::get(&uri).await?.expect("every kind is indexed");
        assert_eq!(post.namespace, namespace);
        assert_eq!(post.kind, kind);
        let row = fetch_row_from_graph(Query::new("test_generic_app_edges", "MATCH (r:Resource {uri: $uri}) RETURN COUNT { (:Resource {uri: $uri}) } AS nodes, COUNT { (:User {id: $author})-[:AUTHORED]->(r) } AS authors, COUNT { (r)-[:APP_REPLIED]->(:Resource) } AS parents, COUNT { (r)-[:APP_EMBEDDED]->(:Resource) } AS embeds").param("uri", uri.clone()).param("author", author.clone())).await?.unwrap();
        for count in ["nodes", "authors", "parents", "embeds"] {
            assert_eq!(row.get::<i64>(count)?, 1, "{namespace}/{kind}: {count}");
        }
        let parsed = AppPostUri::parse(&format!("pubky://{author}{path}")).unwrap();
        let record = ResourceChange::version_record(&parsed).await?.unwrap();
        assert_eq!(record.uri, uri);
        assert_eq!(record.resource_id, post.id);
        assert_eq!(record.kind, ResourceChangeKind::Post);
        test.del(&key, &path).await?;
        assert!(AppPost::get(&uri).await?.is_none());
        assert!(ResourceChange::version_record(&parsed).await?.is_none());
    }
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(
        lines.iter().map(|l| l.op).collect::<Vec<_>>(),
        [
            ResourceChangeOp::Put,
            ResourceChangeOp::Del,
            ResourceChangeOp::Put,
            ResourceChangeOp::Del
        ]
    );
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn arbitrary_namespaces_are_indexed_and_identity_includes_namespace() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let id = generate_post_id();
    let social_path: ResourcePath = format!("/pub/pubky.app/posts/{id}").parse()?;
    let social = short_post("social collision control");
    test.put(&key, &social_path, &social).await?;
    for namespace in [
        "x",
        "new-app2",
        "other-example",
        "abcdefghijklmnopqrstuvwxyz012345",
    ] {
        let path: ResourcePath = format!("/pub/{namespace}/v1/posts/{id}/{id}.json").parse()?;
        test.put(&key, &path, &envelope(&id, "note")).await?;
        assert_eq!(
            AppPost::get(&format!("pubky://{author}/pub/{namespace}/v1/posts/{id}"))
                .await?
                .unwrap()
                .namespace,
            namespace
        );
    }
    assert_eq!(
        PostDetails::get_by_id(&author, &id).await?.unwrap().content,
        social.content
    );
    for namespace in [
        "x",
        "new-app2",
        "other-example",
        "abcdefghijklmnopqrstuvwxyz012345",
    ] {
        test.del(
            &key,
            &format!("/pub/{namespace}/v1/posts/{id}/{id}.json").parse()?,
        )
        .await?;
    }
    assert!(PostDetails::get_by_id(&author, &id).await?.is_some());
    test.del(&key, &social_path).await?;
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn invalid_envelope_is_spec_validation_without_feed_or_node() -> Result<()> {
    let (test, key, author) = setup().await?;
    let mut test = test.remove_event_processing().await;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/example/v1/posts/{id}/{id}.json").parse()?;
    let version_uri = format!("pubky://{author}{path}");
    let uri = AppPostUri::parse(&version_uri).unwrap().uri();
    let mut cases = vec![
        json!({}),
        json!({"kind":"note", "content":null}),
        envelope(&id, "UPPER"),
    ];
    for (field, value) in [
        ("content", json!({})),
        ("parent", json!("relative/path")),
        ("embed", json!({"uri":"https://example.com"})),
        ("attachments", Value::Null),
    ] {
        let mut body = envelope(&id, "note");
        body[field] = value;
        cases.push(body);
    }
    for body in cases {
        test.put(&key, &path, &body).await?;
        let error = retrieve_and_handle_event_line(
            &format!("PUT {version_uri}"),
            test.event_processor_runner.event_handler.clone(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, EventProcessorError::SpecValidation(_)),
            "{error:?}"
        );
        assert!(AppPost::get(&uri).await?.is_none());
    }
    assert!(own_lines(&author, cursor).await?.is_empty());
    test.del(&key, &path).await?;
    test.ensure_event_processing = true;
    // Avoid replaying the deliberately invalid historical events during cleanup.
    retrieve_and_handle_event_line(
        &format!("DEL pubky://{author}/pub/pubky.app/profile.json"),
        test.event_processor_runner.event_handler.clone(),
    )
    .await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn social_reply_to_app_post_and_app_parent_edges() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/example/v1/posts/{id}/{id}.json").parse()?;
    let version_uri = format!("pubky://{author}{path}");
    let uri = AppPostUri::parse(&version_uri).unwrap().uri();
    test.put(&key, &path, &envelope(&id, "note")).await?;
    let reply_id = generate_post_id();
    let reply_path: ResourcePath = format!("/pub/pubky.app/posts/{reply_id}").parse()?;
    let mut reply = short_post("Social reply to an app post");
    reply.parent = Some(uri.clone());
    test.put(&key, &reply_path, &reply).await?;
    assert_eq!(
        PostRelationships::get_by_id(&author, &reply_id)
            .await?
            .unwrap()
            .replied
            .as_deref(),
        Some(uri.as_str())
    );
    let row = fetch_row_from_graph(Query::new("test_social_app_reply", "MATCH (p:Post {id: $id})-[:REPLIED]->(a:Resource {uri: $uri}) RETURN p.app_parent AS parent").param("id", reply_id.clone()).param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<String>("parent")?, uri);
    let child_id = generate_post_id();
    let child_path: ResourcePath =
        format!("/pub/other-example/v1/posts/{child_id}/{child_id}.json").parse()?;
    let mut child = envelope(&child_id, "note");
    child["parent"] = json!(uri);
    child["embed"] = json!(post_uri_builder(author.clone(), reply_id.clone()));
    test.put(&key, &child_path, &child).await?;
    let row = fetch_row_from_graph(Query::new("test_cross_app_edges", "MATCH (p:Resource {uri: $uri}) RETURN COUNT { (p)-[:APP_REPLIED]->(:Resource) } AS parents, COUNT { (p)-[:APP_EMBEDDED]->(:Post) } AS embeds").param("uri", format!("pubky://{author}/pub/other-example/v1/posts/{child_id}"))).await?.unwrap();
    assert_eq!(row.get::<i64>("parents")?, 1);
    assert_eq!(row.get::<i64>("embeds")?, 1);
    let v1_reply_id = generate_post_id();
    let v1_reply_path: ResourcePath =
        format!("/pub/social/v1/posts/{v1_reply_id}/{v1_reply_id}.json").parse()?;
    test.put(
        &key,
        &v1_reply_path,
        &json!({"kind":"note", "content":"A v1 social reply", "parent":uri}),
    )
    .await?;
    assert_eq!(
        PostRelationships::get_by_id(&author, &v1_reply_id)
            .await?
            .unwrap()
            .replied
            .as_deref(),
        Some(uri.as_str())
    );
    let counts = nexus_common::models::post::PostCounts::get_by_id(&author, &reply_id)
        .await?
        .unwrap();
    assert_eq!(
        (counts.replies, counts.reposts),
        (0, 0),
        "app-origin edges do not count as social replies/reposts"
    );
    // App DEL removes app metadata and outgoing edges, preserving incoming replies.
    test.del(&key, &child_path).await?;
    test.del(&key, &path).await?;
    assert!(AppPost::get(&uri).await?.is_none());
    assert!(PostDetails::get_by_id(&author, &reply_id).await?.is_some());
    // Edits and cache recovery after app-parent deletion keep these posts as replies.
    reply.content = "Edited after the app parent was deleted".into();
    test.put(&key, &reply_path, &reply).await?;
    for id in [&reply_id, &v1_reply_id] {
        PostDetails::reindex(&author, id).await?;
        PostRelationships::reindex(&author, id).await?;
        assert_eq!(
            PostRelationships::get_by_id(&author, id)
                .await?
                .unwrap()
                .replied
                .as_deref(),
            Some(uri.as_str())
        );
        assert!(
            nexus_common::models::post::PostCounts::get_from_graph(&author, id)
                .await?
                .unwrap()
                .1
        );
    }
    use nexus_common::db::kv::SortOrder;
    use nexus_common::models::post::{KindFilter, PostStream, StreamSource};
    use nexus_common::types::{Pagination, StreamSorting};
    let replies =
        PostStream::get_author_posts(&author, SortOrder::Descending, None, None, None, None, true)
            .await?;
    assert_eq!(replies.post_keys.len(), 2);
    for kind in [
        None,
        Some(KindFilter::Kind(
            pubky_social_specs::legacy_v0::PubkyAppPostKind::Short,
        )),
    ] {
        assert!(PostStream::get_post_keys(
            StreamSource::Author {
                author_id: author.clone()
            },
            Pagination::default(),
            SortOrder::Descending,
            StreamSorting::Timeline,
            None,
            None,
            kind
        )
        .await?
        .is_none());
    }
    test.del(&key, &v1_reply_path).await?;
    test.del(&key, &reply_path).await?;
    assert!(PostStream::get_author_posts(
        &author,
        SortOrder::Descending,
        None,
        None,
        None,
        None,
        true
    )
    .await?
    .post_keys
    .is_empty());
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn graph_commit_retry_and_pending_delete_recovery() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path = AppPostUri::parse(&format!(
        "pubky://{author}/pub/example/v1/posts/{id}/{id}.json"
    ))
    .unwrap();
    let bytes = serde_json::to_vec(&envelope(&id, "article"))?;
    let post = AppPost::new(&path, path.read(&bytes).unwrap());
    assert!(post.put().await?); // crash boundary: graph committed, feed not appended
    assert!(own_lines(&author, cursor).await?.is_empty());
    app_post::put(&path, &bytes).await?;
    app_post::put(&path, &bytes).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].uri, lines[1].uri);
    assert_eq!(lines[0].object_path, lines[1].object_path);
    assert_eq!(lines[0].resource_id, lines[1].resource_id);
    assert!(
        lines[1].at >= lines[0].at,
        "retries upsert the same version key"
    );
    ResourceChange::post(&path, ResourceChangeOp::Del)?
        .prepare_delete()
        .await?;
    AppPost::delete(&path.uri()).await?; // crash boundary: pending metadata survives
    app_post::del(&path).await?;
    app_post::del(&path).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[2].op, ResourceChangeOp::Del);
    assert_eq!(lines[3].op, ResourceChangeOp::Del);
    assert_eq!(lines[2].object_path, lines[3].object_path);
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn kind_edits_update_metadata_and_edges_on_the_same_resource() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path = AppPostUri::parse(&format!(
        "pubky://{author}/pub/example/v1/posts/{id}/{id}.json"
    ))
    .unwrap();
    let mut original = None;
    for (index, kind) in ["note", "memo", "article"].into_iter().enumerate() {
        let parent = format!("https://example.com/parent/{index}");
        let mut body = envelope(&id, kind);
        body["parent"] = json!(parent);
        if index == 0 {
            body["embed"] = json!("https://example.com/first");
        }
        app_post::put(&path, &serde_json::to_vec(&body)?).await?;
        let post = AppPost::get(&path.uri()).await?.unwrap();
        assert_eq!(post.kind, kind);
        assert_eq!(post.parent.as_deref(), Some(parent.as_str()));
        let identity = (post.id, post.indexed_at);
        assert_eq!(original.get_or_insert(identity.clone()), &identity);
        let row = fetch_row_from_graph(Query::new("test_kind_edit_edges", "MATCH (r:Resource {uri: $uri}) RETURN COUNT { (:User)-[:AUTHORED]->(r) } AS authors, COUNT { (r)-[:APP_REPLIED]->() } AS parents, COUNT { (r)-[:APP_REPLIED]->(:Resource {uri: $parent}) } AS current_parent, COUNT { (r)-[:APP_EMBEDDED]->() } AS embeds").param("uri", path.uri()).param("parent", parent)).await?.unwrap();
        assert_eq!(row.get::<i64>("authors")?, 1);
        assert_eq!(row.get::<i64>("parents")?, 1);
        assert_eq!(row.get::<i64>("current_parent")?, 1);
        assert_eq!(row.get::<i64>("embeds")?, i64::from(index == 0));
        assert_eq!(
            ResourceChange::version_record(&path).await?.unwrap().kind,
            ResourceChangeKind::Post
        );
    }
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 3);
    assert!(lines
        .iter()
        .all(|line| line.op == ResourceChangeOp::Put && line.uri == path.uri()));
    app_post::del(&path).await?;
    assert!(AppPost::get(&path.uri()).await?.is_none());
    assert!(ResourceChange::version_record(&path).await?.is_none());
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn social_put_edit_retry_and_del_do_not_emit_resource_post_lines() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    for kind in ["event", "rsvp", "note"] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/social/v1/posts/{id}/{id}.json").parse()?;
        test.put(&key, &path, &envelope(&id, kind)).await?;
        let edit = generate_post_id();
        let edited: ResourcePath = format!("/pub/social/v1/posts/{id}/{edit}.json").parse()?;
        let mut body = envelope(&id, kind);
        body["content"] = json!("Updated social post");
        test.put(&key, &edited, &body).await?;
        retrieve_and_handle_event_line(
            &format!("PUT pubky://{author}{edited}"),
            test.event_processor_runner.event_handler.clone(),
        )
        .await?;
        assert_eq!(
            PostDetails::get_by_id(&author, &id).await?.unwrap().content,
            "Updated social post"
        );
        let parsed = AppPostUri::parse_social(&format!("pubky://{author}{edited}")).unwrap();
        assert!(ResourceChange::version_record(&parsed).await?.is_none());
        assert!(own_lines(&author, cursor).await?.is_empty());
        test.del(&key, &edited).await?;
        test.del(&key, &path).await?;
        assert!(own_lines(&author, cursor).await?.is_empty());
    }
    test.cleanup_user(&key).await?;
    Ok(())
}

/// A tag or reply may arrive before the app's own object. All three must share
/// the generic URI Resource, including after kind edits and version deletion.
#[tokio_shared_rt::test(shared)]
async fn tags_replies_and_app_metadata_share_one_resource() -> Result<()> {
    use nexus_common::models::resource::view::ResourceView;
    use nexus_common::models::tag::search::TagSearch;
    use nexus_common::universal_tag::normalize::{normalize_uri, resource_id};
    use pubky_social_specs::{
        traits::{HasIdPath, HashId},
        PubkySocialTag,
    };
    let (mut test, key, author) = setup().await?;
    let tagger_key = Keypair::random();
    test.create_user(
        &tagger_key,
        &test_user("App reference", "resource identity"),
    )
    .await?;
    // Exercise both arrival orders, including a pre-existing generic Resource.
    for app_first in [false, true] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/example/v1/posts/{id}/{id}.json").parse()?;
        let uri = format!("pubky://{author}/pub/example/v1/posts/{id}");
        let resource_id = resource_id(&normalize_uri(&uri).unwrap().0);
        if app_first {
            test.put(&key, &path, &envelope(&id, "note")).await?;
        }
        let tag = PubkySocialTag::new(uri.clone(), "app-reference".into());
        let tag_path: ResourcePath = PubkySocialTag::create_path(&tag.create_id()).parse()?;
        test.put(&tagger_key, &tag_path, &tag).await?;
        let before = fetch_row_from_graph(
            Query::new(
                "shared_resource_before",
                "MATCH (r:Resource {id: $id}) RETURN elementId(r) AS element, r.indexed_at AS at",
            )
            .param("id", resource_id.clone()),
        )
        .await?
        .unwrap();
        let element: String = before.get("element")?;
        let indexed_at: i64 = before.get("at")?;
        let reply_id = generate_post_id();
        let reply_path: ResourcePath = format!("/pub/pubky.app/posts/{reply_id}").parse()?;
        let mut reply = short_post("Reply to any app URI, before or after its PUT");
        reply.parent = Some(uri.clone());
        test.put(&tagger_key, &reply_path, &reply).await?;
        test.put(&key, &path, &envelope(&id, "note")).await?;
        let app = AppPost::get(&uri).await?.unwrap();
        assert_eq!(app.id, resource_id);
        assert_eq!(app.post_id, id);
        // Removing the last tag must not delete a Resource owned by an app or a reply.
        test.del(&tagger_key, &tag_path).await?;
        assert_eq!(AppPost::get(&uri).await?.unwrap().id, resource_id);
        test.put(&tagger_key, &tag_path, &tag).await?;
        for kind in ["memo", "article"] {
            test.put(&key, &path, &envelope(&id, kind)).await?;
            assert_eq!(AppPost::get(&uri).await?.unwrap().kind, kind);
            let row = fetch_row_from_graph(Query::new("shared_resource_check", "MATCH (r:Resource {id: $id}) RETURN elementId(r) AS element, r.indexed_at AS at, COUNT { (:User)-[:TAGGED]->(r) } AS tags, COUNT { (:Post)-[:REPLIED]->(r) } AS replies, COUNT { (:User)-[:AUTHORED]->(r) } AS authors").param("id", resource_id.clone())).await?.unwrap();
            assert_eq!(row.get::<String>("element")?, element);
            assert_eq!(row.get::<i64>("at")?, indexed_at);
            assert_eq!(row.get::<i64>("tags")?, 1);
            assert_eq!(row.get::<i64>("replies")?, 1);
            assert_eq!(row.get::<i64>("authors")?, 1);
        }
        test.del(&key, &path).await?;
        assert!(AppPost::get(&uri).await?.is_none());
        let view = ResourceView::get_by_id(&resource_id, None, None, None, None)
            .await?
            .unwrap();
        assert_eq!(view.details.uri, uri);
        assert_eq!(view.tags.len(), 1);
        let row = fetch_row_from_graph(Query::new("shared_resource_deleted", "MATCH (r:Resource {id: $id}) RETURN elementId(r) AS element, COUNT { (:Post)-[:REPLIED]->(r) } AS replies, COUNT { (r)-[:APP_REPLIED|APP_EMBEDDED]->() } AS outgoing, COUNT { (p:AppPost {uri: $uri}) } AS separate_nodes").param("id", resource_id.clone()).param("uri", uri)).await?.unwrap();
        assert_eq!(row.get::<String>("element")?, element);
        assert_eq!(row.get::<i64>("replies")?, 1);
        assert_eq!(row.get::<i64>("outgoing")?, 0);
        assert_eq!(row.get::<i64>("separate_nodes")?, 0);
        test.del(&tagger_key, &tag_path).await?;
        test.del(&tagger_key, &reply_path).await?;
    }
    TagSearch::del_from_index("app-reference").await?;
    test.cleanup_user(&tagger_key).await?;
    test.cleanup_user(&key).await?;
    Ok(())
}
