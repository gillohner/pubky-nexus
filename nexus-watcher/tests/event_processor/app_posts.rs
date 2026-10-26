use super::posts::utils::{short_post, test_user};
use super::utils::watcher::{generate_post_id, retrieve_and_handle_event_line, WatcherTest};
use anyhow::Result;
use nexus_common::config::watcher::default_app_kinds;
use nexus_common::db::{fetch_row_from_graph, graph::Query};
use nexus_common::models::app_post::{AppPost, AppPostUri};
use nexus_common::models::event::app_post_changes::{AppPostChange, AppPostChangeOp};
use nexus_common::models::post::{PostDetails, PostRelationships};
use nexus_watcher::errors::EventProcessorError;
use nexus_watcher::events::handlers::app_post;
use pubky::{Keypair, ResourcePath};
use pubky_social_specs::legacy_v0::post_uri_builder;
use serde_json::{json, Value};

fn envelope(_id: &str, kind: &str) -> Value {
    json!({"kind": kind, "content": "  {uninterpreted app data}  ", "attachments": [], "ext": {"anything": [null, true, 7]}})
}

async fn own_lines(author: &str, mut cursor: u64) -> Result<Vec<AppPostChange>> {
    let mut lines = Vec::new();
    loop {
        let (page, _, _) = AppPostChange::get_from_index(cursor, 1000).await?;
        if page.lines.is_empty() {
            break;
        }
        cursor = page.next_cursor;
        lines.extend(page.lines.into_iter().filter(|line| line.author == author));
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
async fn durable_put_edges_edit_and_del() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = AppPostChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/music/v1/posts/{id}/{id}.json").parse()?;
    let version_uri = format!("pubky://{author}{path}");
    let uri = AppPostUri::parse(&version_uri).unwrap().uri();
    let mut body = envelope(&id, "listen");
    body["parent"] = json!(format!("pubky://{author}/pub/album/catalog.json"));
    body["embed"] = json!("https://example.com/album");
    test.put(&key, &path, &body).await?;
    let post = AppPost::get(&uri).await?.expect("durable app post");
    assert_eq!(post.namespace, "music");
    assert_eq!(post.kind, "listen");
    let row = fetch_row_from_graph(Query::new("test_app_edges", "MATCH (u:User)-[:AUTHORED]->(p:AppPost {uri: $uri}) RETURN u.id AS author, p.content IS NULL AND p.ext IS NULL AS opaque, COUNT { (p)-[:APP_REPLIED]->(:Resource) } AS parents, COUNT { (p)-[:APP_EMBEDDED]->(:Resource) } AS embeds").param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<String>("author")?, author);
    assert!(row.get::<bool>("opaque")?);
    assert_eq!(row.get::<i64>("parents")?, 1);
    assert_eq!(row.get::<i64>("embeds")?, 1);
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 1);
    assert_eq!(
        serde_json::to_value(&lines[0])?,
        json!({"op":"put", "author":author, "post":format!("/pub/music/v1/posts/{id}"), "version": path.to_string(), "namespace":"music", "post_kind":"listen", "at":lines[0].at})
    );

    body["parent"] = Value::Null;
    body["embed"] = Value::Null;
    body["kind"] = json!("session");
    let edit = generate_post_id();
    let edited_path: ResourcePath =
        format!("/pub/music/v1/posts/{id}/{edit}-listen.json").parse()?;
    test.put(&key, &edited_path, &body).await?;
    assert_eq!(
        AppPost::get(&uri).await?.unwrap().version_uri,
        format!("pubky://{author}{edited_path}")
    );
    let row = fetch_row_from_graph(Query::new("test_app_edit", "MATCH (p:AppPost {uri: $uri}) RETURN COUNT { (p)-[:APP_REPLIED|APP_EMBEDDED]->() } AS edges").param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<i64>("edges")?, 0);
    test.del(&key, &path).await?;
    assert!(AppPost::get(&uri).await?.is_none());
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2].op, AppPostChangeOp::Del);
    assert_eq!(lines[2].post_kind, "listen");
    assert_eq!(lines[2].version.as_deref(), Some(path.to_string().as_str()));
    // N3 removes the node on the first DEL, but every stored version is reported.
    test.del(&key, &edited_path).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[3].post_kind, "session");
    assert_eq!(
        lines[3].version.as_deref(),
        Some(edited_path.to_string().as_str())
    );
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn music_listening_is_feed_only_but_listen_history_is_durable() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = AppPostChange::get_from_index(0, 0).await?;
    for kind in ["listening", "listen"] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/music/v1/posts/{id}/{id}.json").parse()?;
        let uri = format!("pubky://{author}/pub/music/v1/posts/{id}");
        test.put(&key, &path, &envelope(&id, kind)).await?;
        assert_eq!(AppPost::get(&uri).await?.is_some(), kind == "listen");
        let parsed = AppPostUri::parse(&format!("pubky://{author}{path}")).unwrap();
        assert!(AppPostChange::version_record(&parsed).await?.is_some());
        // Feed-only is an indexing tier, not a homeserver deletion or TTL.
        test.del(&key, &path).await?;
        assert!(AppPost::get(&uri).await?.is_none());
        assert!(AppPostChange::version_record(&parsed).await?.is_none());
    }
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(
        lines.iter().map(|l| l.op).collect::<Vec<_>>(),
        [
            AppPostChangeOp::Put,
            AppPostChangeOp::Del,
            AppPostChangeOp::Put,
            AppPostChangeOp::Del
        ]
    );
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn unknown_namespace_is_durable_and_identity_includes_namespace() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let id = generate_post_id();
    let social_path: ResourcePath = format!("/pub/pubky.app/posts/{id}").parse()?;
    let social = short_post("social collision control");
    test.put(&key, &social_path, &social).await?;
    for namespace in ["new.app", "marketplace"] {
        let path: ResourcePath = format!("/pub/{namespace}/v1/posts/{id}/{id}.json").parse()?;
        test.put(&key, &path, &envelope(&id, "listen")).await?;
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
    for namespace in ["new.app", "marketplace"] {
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
    let (_, _, cursor) = AppPostChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/maps/v1/posts/{id}/{id}.json").parse()?;
    let version_uri = format!("pubky://{author}{path}");
    let uri = AppPostUri::parse(&version_uri).unwrap().uri();
    let mut cases = vec![
        json!({}),
        json!({"kind":"route", "content":null}),
        envelope(&id, "UPPER"),
    ];
    for (field, value) in [
        ("content", json!({})),
        ("parent", json!("relative/path")),
        ("embed", json!({"uri":"https://example.com"})),
        ("attachments", Value::Null),
    ] {
        let mut body = envelope(&id, "route");
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
    let path: ResourcePath = format!("/pub/maps/v1/posts/{id}/{id}.json").parse()?;
    let version_uri = format!("pubky://{author}{path}");
    let uri = AppPostUri::parse(&version_uri).unwrap().uri();
    test.put(&key, &path, &envelope(&id, "route")).await?;
    let reply_id = generate_post_id();
    let reply_path: ResourcePath = format!("/pub/pubky.app/posts/{reply_id}").parse()?;
    let mut reply = short_post("Social reply to an app route");
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
    let row = fetch_row_from_graph(Query::new("test_social_app_reply", "MATCH (p:Post {id: $id})-[:REPLIED]->(a:AppPost {uri: $uri}) RETURN p.app_parent AS parent").param("id", reply_id.clone()).param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<String>("parent")?, uri);
    let child_id = generate_post_id();
    let child_path: ResourcePath =
        format!("/pub/events/v1/posts/{child_id}/{child_id}.json").parse()?;
    let mut child = envelope(&child_id, "availability");
    child["parent"] = json!(uri);
    child["embed"] = json!(post_uri_builder(author.clone(), reply_id.clone()));
    test.put(&key, &child_path, &child).await?;
    let row = fetch_row_from_graph(Query::new("test_cross_app_edges", "MATCH (p:AppPost {uri: $uri}) RETURN COUNT { (p)-[:APP_REPLIED]->(:AppPost) } AS parents, COUNT { (p)-[:APP_EMBEDDED]->(:Post) } AS embeds").param("uri", format!("pubky://{author}/pub/events/v1/posts/{child_id}"))).await?.unwrap();
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
    // App DEL removes all its edges even when it had a social reply.
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
    let (_, _, cursor) = AppPostChange::get_from_index(0, 0).await?;
    let id = generate_post_id();
    let path = AppPostUri::parse(&format!(
        "pubky://{author}/pub/music/v1/posts/{id}/{id}.json"
    ))
    .unwrap();
    let bytes = serde_json::to_vec(&envelope(&id, "session"))?;
    let post = AppPost::new(&path, path.read(&bytes).unwrap());
    assert!(post.put().await?); // crash boundary: graph committed, feed not appended
    assert!(own_lines(&author, cursor).await?.is_empty());
    app_post::put(&path, &bytes, &default_app_kinds()).await?;
    app_post::put(&path, &bytes, &default_app_kinds()).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].post, lines[1].post);
    assert_eq!(lines[0].version, lines[1].version);
    assert_eq!(lines[0].post_kind, lines[1].post_kind);
    assert!(
        lines[1].at >= lines[0].at,
        "retries upsert the same version key"
    );
    AppPostChange::new(&path, "session".into(), AppPostChangeOp::Del)
        .prepare_delete()
        .await?;
    AppPost::delete(&path.uri()).await?; // crash boundary: pending metadata survives
    app_post::del(&path).await?;
    app_post::del(&path).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2].op, AppPostChangeOp::Del);
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn kind_edits_and_operator_config_change_the_tier_without_stale_metadata() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let id = generate_post_id();
    let path = AppPostUri::parse(&format!(
        "pubky://{author}/pub/music/v1/posts/{id}/{id}.json"
    ))
    .unwrap();
    let defaults = default_app_kinds();
    for kind in ["listen", "listening", "session"] {
        let body = serde_json::to_vec(&envelope(&id, kind))?;
        app_post::put(&path, &body, &defaults).await?;
        assert_eq!(
            AppPost::get(&path.uri()).await?.is_some(),
            kind != "listening"
        );
        assert_eq!(
            AppPostChange::version_record(&path)
                .await?
                .unwrap()
                .post_kind,
            kind
        );
    }
    let configured = std::collections::BTreeMap::from([(
        "music".into(),
        nexus_common::config::watcher::AppKindsConfig {
            feed_only: vec!["session".into()],
        },
    )]);
    app_post::put(
        &path,
        &serde_json::to_vec(&envelope(&id, "session"))?,
        &configured,
    )
    .await?;
    assert!(AppPost::get(&path.uri()).await?.is_none());
    assert!(AppPostChange::version_record(&path).await?.is_some());
    app_post::del(&path).await?;
    assert!(AppPostChange::version_record(&path).await?.is_none());
    test.cleanup_user(&key).await?;
    Ok(())
}
