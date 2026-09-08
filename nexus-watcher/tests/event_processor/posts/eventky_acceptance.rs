//! Real homeserver events exercise custom posts through the ordinary social graph.
use anyhow::{Context, Result};
use nexus_common::{
    db::graph::setup::setup_graph,
    models::{
        post::{Bookmark, PostCounts, PostDetails, PostRelationships},
        post_projection::{self as projection, ChangesRequest},
        tag::{post::TagPost, traits::TagCollection},
    },
    StackConfig,
};
use pubky::{Keypair, ResourcePath};
use pubky_app_specs::{traits::HashId, PubkyAppBookmark, PubkyAppTag, PubkyAppUser};
use serde_json::{json, Value};

use crate::event_processor::{
    tags::utils::find_post_tag,
    utils::watcher::{generate_post_id, HomeserverHashIdPath, WatcherTest},
};

struct Actor {
    key: Keypair,
    id: String,
}

#[derive(Clone)]
struct SourcePost {
    id: String,
    uri: String,
    path: ResourcePath,
    kind: &'static str,
    content: String,
}

struct Interactions {
    reply: SourcePost,
    repost: SourcePost,
    tag_path: ResourcePath,
    bookmark_path: ResourcePath,
    bookmark_id: String,
}

#[tokio_shared_rt::test(shared)]
#[ignore = "requires explicit isolated Neo4j, Redis and PostgreSQL test services"]
async fn event_and_calendar_homeserver_social_lifecycle() -> Result<()> {
    let files = tempfile::tempdir()?;
    let stack = isolated_stack(files.path())?;
    let mut test = WatcherTest::setup_with_stack(None, stack).await?;
    setup_graph().await?;
    let owner = actor(&mut test, "Eventky owner").await?;
    let reader = actor(&mut test, "Eventky reader").await?;
    let before = projection::head().await?;
    let calendar = source(&owner, "calendar", calendar_content(&reader.id))?;
    let event = source(&owner, "event", event_content(&calendar.uri))?;
    let mut histories = Vec::new();

    // Both are ordinary post paths; calendar membership stays opaque to Nexus.
    for post in [&calendar, &event] {
        test.put(&owner.key, &post.path, envelope(post)).await?;
        assert_source(&owner.id, post, &post.content).await?;
        assert_kind_only_edits(&mut test, &owner, post).await?;
    }
    for post in [&event, &calendar] {
        let interactions = interact(&mut test, &reader, post).await?;
        assert_interactions(&owner.id, &reader, (post, &interactions)).await?;
        let edited = edited_content(post)?;
        let mut replacement = envelope(post);
        replacement["content"] = edited.clone().into();
        test.put(&owner.key, &post.path, replacement).await?;
        assert_source(&owner.id, post, &edited).await?;
        assert_interactions(&owner.id, &reader, (post, &interactions)).await?;
        remove_interactions(&mut test, &reader, &interactions).await?;
        assert_counts(&owner.id, &post.id, 0).await?;
        assert!(Bookmark::get_from_graph(&owner.id, &post.id, &reader.id)
            .await?
            .is_none());
        test.del(&owner.key, &post.path).await?;
        assert!(PostDetails::get_from_graph(&owner.id, &post.id)
            .await?
            .is_none());
        assert!(PostDetails::get_from_index(&owner.id, &post.id)
            .await?
            .is_none());
        histories.push((post, edited));
    }
    assert_source_history(&before, &histories).await?;
    test.cleanup_user(&reader.key).await?;
    test.cleanup_user(&owner.key).await?;
    Ok(())
}

fn isolated_stack(files: &std::path::Path) -> Result<StackConfig> {
    let postgres = std::env::var("TEST_PUBKY_CONNECTION_STRING")?;
    anyhow::ensure!(
        postgres.contains("pubky-test=true"),
        "PostgreSQL must use an ephemeral test database"
    );
    let mut stack = StackConfig {
        files_path: files.to_path_buf(),
        ..Default::default()
    };
    stack.db.neo4j.uri = std::env::var("EVENTKY_TEST_NEO4J_URI")?;
    stack.db.redis = std::env::var("EVENTKY_TEST_REDIS_URI")?;
    Ok(stack)
}

async fn actor(test: &mut WatcherTest, name: &str) -> Result<Actor> {
    let key = Keypair::random();
    let profile = PubkyAppUser {
        name: name.into(),
        bio: None,
        image: None,
        links: None,
        status: None,
    };
    let id = test.create_user(&key, &profile).await?;
    Ok(Actor { key, id })
}

fn source(author: &Actor, kind: &'static str, content: Value) -> Result<SourcePost> {
    let id = generate_post_id();
    let path: ResourcePath = format!("/pub/pubky.app/posts/{id}").parse()?;
    Ok(SourcePost {
        uri: format!("pubky://{}/pub/pubky.app/posts/{id}", author.id),
        id,
        path,
        kind,
        content: match content {
            Value::String(text) => text,
            other => other.to_string(),
        },
    })
}

fn envelope(post: &SourcePost) -> Value {
    json!({"kind":post.kind,"content":post.content,"parent":null,"embed":null,"attachments":null,"lock":null})
}

fn calendar_content(contributor: &str) -> Value {
    json!({
        "schema":"eventky.calendar", "schema_version":1,
        "uid":"urn:uuid:63a59806-5bf0-44f0-bf26-041f1d5a4a95",
        "name":"Pubky builders", "timezone":"Europe/Zurich", "color":"#6757E8",
        "contributors":[contributor], "created":"2026-09-08T09:00:00Z",
        "last_modified":"2026-09-08T09:00:00Z", "sequence":0,
        "extensions":{"opaque":{"keep":[1,true,"calendar"]}}
    })
}

fn event_content(calendar_uri: &str) -> Value {
    json!({
        "schema":"eventky.event", "schema_version":1,
        "uid":"urn:uuid:84194bcd-17a2-43ae-99b9-2212e8b791c8",
        "summary":"Pubky builders meetup", "description":"Native comments and tags.",
        "dtstart":{"type":"zoned","value":"2026-10-01T18:00:00","tzid":"Europe/Zurich"},
        "duration":"PT2H", "dtstamp":"2026-09-08T09:00:00Z",
        "created":"2026-09-08T09:00:00Z", "last_modified":"2026-09-08T09:00:00Z",
        "sequence":0, "status":"CONFIRMED", "rrule":"FREQ=WEEKLY;BYDAY=TH;COUNT=6",
        "calendar_uris":[calendar_uri], "extensions":{"opaque":{"keep":[1,true,"event"]}}
    })
}

fn edited_content(post: &SourcePost) -> Result<String> {
    let mut content: Value = serde_json::from_str(&post.content)?;
    content["sequence"] = 1.into();
    content["last_modified"] = "2026-09-08T10:00:00Z".into();
    content[if post.kind == "event" {
        "summary"
    } else {
        "name"
    }] = "Updated native post".into();
    Ok(content.to_string())
}

async fn interact(
    test: &mut WatcherTest,
    reader: &Actor,
    post: &SourcePost,
) -> Result<Interactions> {
    let reply = source(reader, "short", json!("An ordinary comment"))?;
    let repost = source(reader, "short", json!("An ordinary repost"))?;
    let mut reply_wire = envelope(&reply);
    reply_wire["parent"] = post.uri.clone().into();
    let mut repost_wire = envelope(&repost);
    repost_wire["embed"] = post.uri.clone().into();
    test.put(&reader.key, &reply.path, reply_wire).await?;
    test.put(&reader.key, &repost.path, repost_wire).await?;
    let tag = PubkyAppTag {
        uri: post.uri.clone(),
        label: "native-eventky".into(),
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    let bookmark = PubkyAppBookmark {
        uri: post.uri.clone(),
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    let tag_path = tag.hs_path();
    let bookmark_path = bookmark.hs_path();
    let bookmark_id = bookmark.create_id();
    test.put(&reader.key, &tag_path, tag).await?;
    test.put(&reader.key, &bookmark_path, bookmark).await?;
    Ok(Interactions {
        reply,
        repost,
        tag_path,
        bookmark_path,
        bookmark_id,
    })
}

async fn assert_source(owner: &str, post: &SourcePost, content: &str) -> Result<()> {
    let graph = PostDetails::get_from_graph(owner, &post.id)
        .await?
        .context("source graph post")?
        .0;
    let cached = PostDetails::get_from_index(owner, &post.id)
        .await?
        .context("source cached post")?;
    for details in [graph, cached] {
        assert_eq!(details.kind.as_str(), post.kind);
        assert_eq!(details.content, content);
        assert_eq!(details.uri, post.uri);
    }
    Ok(())
}

async fn assert_kind_only_edits(
    test: &mut WatcherTest,
    owner: &Actor,
    post: &SourcePost,
) -> Result<()> {
    let alternate = SourcePost {
        kind: alternate_kind(post),
        ..post.clone()
    };
    for version in [&alternate, post] {
        test.put(&owner.key, &post.path, envelope(version)).await?;
        assert_source(&owner.id, version, &post.content).await?;
    }
    Ok(())
}

fn alternate_kind(post: &SourcePost) -> &'static str {
    if post.kind == "event" {
        "Event"
    } else {
        "Calendar"
    }
}

async fn assert_interactions(
    owner: &str,
    reader: &Actor,
    target: (&SourcePost, &Interactions),
) -> Result<()> {
    let (post, interactions) = target;
    for (id, reply) in [
        (&interactions.reply.id, true),
        (&interactions.repost.id, false),
    ] {
        let graph = PostRelationships::get_from_graph(&reader.id, id)
            .await?
            .context("social graph relationship")?;
        let cache = PostRelationships::get_from_index(&reader.id, id)
            .await?
            .context("social cached relationship")?;
        for relationships in [graph, cache] {
            let uri = if reply {
                relationships.replied
            } else {
                relationships.reposted
            };
            assert_eq!(
                uri.context("native target")?
                    .try_to_uri_str()
                    .map_err(anyhow::Error::msg)?,
                post.uri
            );
        }
    }
    assert_counts(owner, &post.id, 1).await?;
    assert_tags(owner, &reader.id, &post.id).await?;
    for bookmark in [
        Bookmark::get_from_graph(owner, &post.id, &reader.id).await?,
        Bookmark::get_from_index(owner, &post.id, &reader.id).await?,
    ] {
        assert_eq!(
            bookmark.context("native bookmark")?.id,
            interactions.bookmark_id
        );
    }
    Ok(())
}

async fn assert_counts(owner: &str, id: &str, expected: u32) -> Result<()> {
    let graph = PostCounts::get_from_graph(owner, id)
        .await?
        .context("graph counts")?
        .0;
    let cached = PostCounts::get_by_id(owner, id)
        .await?
        .context("cached counts")?;
    for counts in [graph, cached] {
        assert_eq!(
            (
                counts.replies,
                counts.reposts,
                counts.tags,
                counts.unique_tags
            ),
            (expected, expected, expected, expected)
        );
    }
    Ok(())
}

async fn assert_tags(owner: &str, reader: &str, id: &str) -> Result<()> {
    let graph = find_post_tag(owner, id, "native-eventky")
        .await?
        .context("native graph tag")?;
    let cached = TagPost::get_from_index(owner, Some(id), None, None, None, None, false)
        .await?
        .context("native cached tags")?;
    assert_eq!(cached.len(), 1);
    for tag in [graph, cached[0].clone()] {
        assert_eq!(tag.label, "native-eventky");
        assert_eq!(tag.taggers, vec![reader.to_owned()]);
    }
    Ok(())
}

async fn remove_interactions(
    test: &mut WatcherTest,
    reader: &Actor,
    interactions: &Interactions,
) -> Result<()> {
    for path in [
        &interactions.tag_path,
        &interactions.bookmark_path,
        &interactions.reply.path,
        &interactions.repost.path,
    ] {
        test.del(&reader.key, path).await?;
    }
    Ok(())
}

async fn assert_source_history(
    before: &projection::Checkpoint,
    histories: &[(&SourcePost, String)],
) -> Result<()> {
    let page = projection::changes(ChangesRequest {
        epoch: &before.epoch,
        after: projection::parse_revision(&before.revision)?,
        through: None,
        limit: 100,
    })
    .await?;
    assert!(
        page.caught_up,
        "bounded fixture should fit in one source page"
    );
    for (post, edited) in histories {
        let changes: Vec<_> = page
            .items
            .iter()
            .filter(|change| change.uri == post.uri)
            .collect();
        assert_eq!(
            changes.len(),
            5,
            "create, two kind changes, content edit and deletion for {}",
            post.kind
        );
        let versions = [
            (post.kind, post.content.as_str()),
            (alternate_kind(post), post.content.as_str()),
            (post.kind, post.content.as_str()),
            (post.kind, edited.as_str()),
        ];
        for (change, (kind, content)) in changes[..4].iter().zip(versions) {
            let source = change.post.as_ref().context("immutable PUT payload")?;
            assert_eq!(source.kind, kind);
            assert_eq!(source.content, content);
            assert!(source.parent.is_none() && source.embed.is_none() && source.lock.is_none());
        }
        assert!(changes[4].post.is_none(), "durable source deletion");
    }
    Ok(())
}
