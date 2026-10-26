use super::posts::utils::{short_post, test_user};
use super::utils::watcher::{generate_post_id, retrieve_and_handle_event_line, WatcherTest};
use anyhow::Result;
use nexus_common::config::watcher::default_app_kinds;
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
async fn durable_put_edges_edit_and_del() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
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
    body["kind"] = json!("session");
    let edit = generate_post_id();
    let edited_path: ResourcePath =
        format!("/pub/music/v1/posts/{id}/{edit}-listen.json").parse()?;
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

#[tokio_shared_rt::test(shared)]
async fn music_listening_is_feed_only_but_listen_history_is_durable() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    for kind in ["listening", "listen"] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/music/v1/posts/{id}/{id}.json").parse()?;
        let uri = format!("pubky://{author}/pub/music/v1/posts/{id}");
        test.put(&key, &path, &envelope(&id, kind)).await?;
        assert_eq!(AppPost::get(&uri).await?.is_some(), kind == "listen");
        let parsed = AppPostUri::parse(&format!("pubky://{author}{path}")).unwrap();
        assert!(ResourceChange::version_record(&parsed).await?.is_some());
        if kind == "listening" {
            let row = fetch_row_from_graph(
                Query::new(
                    "test_feed_only_no_resource",
                    "MATCH (r:Resource {uri: $uri}) RETURN count(r) AS nodes",
                )
                .param("uri", uri.clone()),
            )
            .await?
            .unwrap();
            assert_eq!(row.get::<i64>("nodes")?, 0);
        }
        // Feed-only is an indexing tier, not a homeserver deletion or TTL.
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
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
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
    let row = fetch_row_from_graph(Query::new("test_social_app_reply", "MATCH (p:Post {id: $id})-[:REPLIED]->(a:Resource {uri: $uri}) RETURN p.app_parent AS parent").param("id", reply_id.clone()).param("uri", uri.clone())).await?.unwrap();
    assert_eq!(row.get::<String>("parent")?, uri);
    let child_id = generate_post_id();
    let child_path: ResourcePath =
        format!("/pub/events/v1/posts/{child_id}/{child_id}.json").parse()?;
    let mut child = envelope(&child_id, "availability");
    child["parent"] = json!(uri);
    child["embed"] = json!(post_uri_builder(author.clone(), reply_id.clone()));
    test.put(&key, &child_path, &child).await?;
    let row = fetch_row_from_graph(Query::new("test_cross_app_edges", "MATCH (p:Resource {uri: $uri}) RETURN COUNT { (p)-[:APP_REPLIED]->(:Resource) } AS parents, COUNT { (p)-[:APP_EMBEDDED]->(:Post) } AS embeds").param("uri", format!("pubky://{author}/pub/events/v1/posts/{child_id}"))).await?.unwrap();
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
            ResourceChange::version_record(&path).await?.unwrap().kind,
            ResourceChangeKind::Post
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
    assert!(ResourceChange::version_record(&path).await?.is_some());
    app_post::del(&path).await?;
    assert!(ResourceChange::version_record(&path).await?.is_none());
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn selected_social_kinds_stay_social_and_emit_version_references() -> Result<()> {
    let (mut test, key, author) = setup().await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let mut paths = Vec::new();
    for kind in ["event", "rsvp", "note"] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/social/v1/posts/{id}/{id}.json").parse()?;
        let body = json!({"kind":kind, "content":"{opaque event/venue data}", "ext":{"secret":"not in line"}});
        test.put(&key, &path, &body).await?;
        let social = PostDetails::get_by_id(&author, &id)
            .await?
            .expect("still indexed as social");
        assert_eq!(social.content, body["content"].as_str().unwrap());
        assert!(
            AppPost::get(&format!("pubky://{author}/pub/social/v1/posts/{id}"))
                .await?
                .is_none()
        );
        paths.push((path, id));
    }
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 2);
    for index in 0..2 {
        assert_eq!(lines[index].kind, ResourceChangeKind::Post);
        assert_eq!(
            lines[index].object_path.as_str(),
            paths[index].0.to_string().as_str()
        );
        assert_eq!(
            lines[index].uri,
            format!("pubky://{author}/pub/social/v1/posts/{}", paths[index].1)
        );
        let json = serde_json::to_value(&lines[index])?;
        assert_eq!(json.as_object().unwrap().len(), 7);
        assert!(json.get("content").is_none());
        assert!(json.get("ext").is_none());
    }
    for (path, _) in paths {
        test.del(&key, &path).await?;
    }
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[2].op, ResourceChangeOp::Del);
    assert_eq!(lines[3].kind, ResourceChangeKind::Post);
    test.cleanup_user(&key).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn social_kind_selection_and_graph_commit_retry() -> Result<()> {
    use nexus_common::config::watcher::PostFeedConfig;
    use nexus_watcher::events::{DefaultEventHandler, Event, EventHandler, ParseResult};
    use std::sync::Arc;
    let (test, key, author) = setup().await?;
    let mut test = test.remove_event_processing().await;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let mut config = nexus_common::WatcherConfig {
        post_feed: PostFeedConfig {
            social_kinds: vec![],
        },
        ..Default::default()
    };
    let client = test.event_processor_runner.client.clone();
    let disabled = DefaultEventHandler::from_config(&config, client.clone());
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/social/v1/posts/{id}/{id}.json").parse()?;
    let uri = format!("pubky://{author}{path}");
    test.put(&key, &path, &envelope(&id, "event")).await?;
    let ParseResult::Parsed(event) = Event::parse_event(&format!("PUT {uri}"))? else {
        panic!("parsed");
    };
    disabled.handle(&event).await?;
    assert!(PostDetails::get_by_id(&author, &id).await?.is_some());
    assert!(own_lines(&author, cursor).await?.is_empty());
    // Models graph commit without append. The default handler must append on retry
    // even though the social node already exists and its content is unchanged.
    let enabled = Arc::new(DefaultEventHandler::from_config(
        &nexus_common::WatcherConfig::default(),
        client.clone(),
    ));
    enabled.handle(&event).await?;
    enabled.handle(&event).await?;
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].uri, lines[1].uri);
    assert_eq!(lines[0].object_path, lines[1].object_path);
    let parsed = AppPostUri::parse_social(&uri).unwrap();
    nexus_watcher::events::handlers::post_feed::before_del(&parsed).await?;
    // Simulate interrupted delete after graph/cache removal but before append.
    let ingestor = nexus_common::utils::test_utils::default_ingestor_tests();
    nexus_watcher::events::handlers::post::del(
        nexus_watcher::events::inputs::ResourceKey {
            owner_id: parsed.author.clone(),
            resource_id: id.clone(),
        },
        &ingestor,
    )
    .await?;
    retrieve_and_handle_event_line(&format!("DEL {uri}"), enabled.clone()).await?;
    retrieve_and_handle_event_line(&format!("DEL {uri}"), enabled.clone()).await?;
    assert_eq!(own_lines(&author, cursor).await?.len(), 3);
    assert!(ResourceChange::version_record(&parsed).await?.is_none());
    test.del(&key, &path).await?;
    // Explicit selection replaces the defaults, without changing social indexing.
    config.post_feed.social_kinds = vec!["note".into()];
    let selected = Arc::new(DefaultEventHandler::from_config(&config, client));
    for kind in ["note", "rsvp"] {
        let id = generate_post_id();
        let path: ResourcePath = format!("/pub/social/v1/posts/{id}/{id}.json").parse()?;
        test.put(&key, &path, &envelope(&id, kind)).await?;
        retrieve_and_handle_event_line(&format!("PUT pubky://{author}{path}"), selected.clone())
            .await?;
        assert!(PostDetails::get_by_id(&author, &id).await?.is_some());
        test.del(&key, &path).await?;
        retrieve_and_handle_event_line(&format!("DEL pubky://{author}{path}"), selected.clone())
            .await?;
    }
    let lines = own_lines(&author, cursor).await?;
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[3].kind, ResourceChangeKind::Post);
    assert_eq!(lines[4].op, ResourceChangeOp::Del);
    test.cleanup_user(&key).await?;
    Ok(())
}

/// A tag or reply may arrive before the app's own object. All three must share
/// the generic URI Resource, including after tier changes and version deletion.
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
        let path: ResourcePath = format!("/pub/music/v1/posts/{id}/{id}.json").parse()?;
        let uri = format!("pubky://{author}/pub/music/v1/posts/{id}");
        let resource_id = resource_id(&normalize_uri(&uri).unwrap().0);
        if app_first {
            test.put(&key, &path, &envelope(&id, "listen")).await?;
        }
        let tag = PubkySocialTag::new(uri.clone(), "app-route".into());
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
        test.put(&key, &path, &envelope(&id, "listen")).await?;
        let app = AppPost::get(&uri).await?.unwrap();
        assert_eq!(app.id, resource_id);
        assert_eq!(app.post_id, id);
        // Removing the last tag must not delete a Resource owned by an app or a reply.
        test.del(&tagger_key, &tag_path).await?;
        assert_eq!(AppPost::get(&uri).await?.unwrap().id, resource_id);
        test.put(&tagger_key, &tag_path, &tag).await?;
        for kind in ["listening", "session"] {
            test.put(&key, &path, &envelope(&id, kind)).await?;
            assert_eq!(AppPost::get(&uri).await?.is_some(), kind == "session");
            let row = fetch_row_from_graph(Query::new("shared_resource_check", "MATCH (r:Resource {id: $id}) RETURN elementId(r) AS element, r.indexed_at AS at, COUNT { (:User)-[:TAGGED]->(r) } AS tags, COUNT { (:Post)-[:REPLIED]->(r) } AS replies, COUNT { (:User)-[:AUTHORED]->(r) } AS authors").param("id", resource_id.clone())).await?.unwrap();
            assert_eq!(row.get::<String>("element")?, element);
            assert_eq!(row.get::<i64>("at")?, indexed_at);
            assert_eq!(row.get::<i64>("tags")?, 1);
            assert_eq!(row.get::<i64>("replies")?, 1);
            assert_eq!(row.get::<i64>("authors")?, i64::from(kind == "session"));
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
    TagSearch::del_from_index("app-route").await?;
    test.cleanup_user(&tagger_key).await?;
    test.cleanup_user(&key).await?;
    Ok(())
}
