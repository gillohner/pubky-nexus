use super::posts::utils::test_user;
use super::utils::watcher::{generate_post_id, WatcherTest};
use anyhow::Result;
use chrono::{Duration, Utc};
use nexus_common::{
    db::{fetch_row_from_graph, graph::Query},
    models::{
        event::resource_changes::ResourceChange,
        notification::{Notification, NotificationBody},
        post::PostDetails,
        social_event::{self, EventQuery},
    },
    types::Pagination,
};
use pubky::{Keypair, ResourcePath};
use serde_json::{json, Value};

fn content() -> Value {
    json!({"title":"CalendarNativeTitle","description":"NativeDescriptionUnique","start":(Utc::now()+Duration::days(2)).format("%Y-%m-%dT%H:%M:%S").to_string(),"timeZone":"UTC","duration":"PT1H","locations":[{"uri":"https://example.com/venue"}],"attendance":{"policy":"open","capacity":2,"waitlist":true}})
}
fn path(id: &str) -> ResourcePath {
    format!("/pub/social/v1/posts/{id}/{id}.json")
        .parse()
        .unwrap()
}
fn post(kind: &str, content: &Value, parent: Option<&str>) -> Value {
    json!({"kind":kind,"content":content.to_string(),"parent":parent,"attachments":[]})
}
async fn put(
    test: &mut WatcherTest,
    key: &Keypair,
    kind: &str,
    body: &Value,
    parent: Option<&str>,
) -> Result<(String, ResourcePath, String)> {
    let id = generate_post_id();
    let path = path(&id);
    test.put(key, &path, &post(kind, body, parent)).await?;
    let uri = pubky_social_specs::post_uri_builder(key.public_key().to_z32(), id.clone());
    Ok((id, path, uri))
}
fn query(author: &str) -> EventQuery {
    EventQuery {
        from: Utc::now().timestamp_millis(),
        to: social_event::calendar::window_end(Utc::now()),
        author: Some(author.into()),
        limit: 200,
        ..Default::default()
    }
}

#[tokio_shared_rt::test(shared)]
async fn native_event_rsvps_guests_retries_and_change_notifications() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Native event owner", ""))
        .await?;
    let alice = Keypair::random();
    let a = test
        .create_user(&alice, &test_user("Native attendee", ""))
        .await?;
    let bob = Keypair::random();
    let b = test
        .create_user(&bob, &test_user("Native requester", ""))
        .await?;
    let (_, _, cursor) = ResourceChange::get_from_index(0, 0).await?;
    let mut body = content();
    let (id, event_path, uri) = put(&mut test, &owner, "event", &body, None).await?;
    let view = social_event::event_view(&author, &id, None).await?.unwrap();
    assert_eq!(view.occurrences.len(), 1);
    assert_eq!(view.end_at - view.start_at, 3_600_000);
    // A delayed non-event projection with identical content cannot erase a newer event kind.
    social_event::sync(
        &author,
        &id,
        &body.to_string(),
        None,
        &social_event::SocialContent::Other,
    )
    .await?;
    assert!(social_event::event_view(&author, &id, None)
        .await?
        .is_some());
    let row = fetch_row_from_graph(
        Query::new(
            "test_locations",
            "MATCH (p:Post {id:$id})-[:LOCATED_AT]->(r:Resource) RETURN r.uri AS uri",
        )
        .param("id", id.clone()),
    )
    .await?
    .unwrap();
    assert_eq!(row.get::<String>("uri")?, "https://example.com/venue");
    let (rsvp_id, rsvp_path, _) = put(
        &mut test,
        &alice,
        "rsvp",
        &json!({"status":"accepted","guests":2}),
        Some(&uri),
    )
    .await?;
    put(
        &mut test,
        &bob,
        "rsvp",
        &json!({"status":"requested","guests":5}),
        Some(&uri),
    )
    .await?;
    let view = social_event::event_view(&author, &id, None).await?.unwrap();
    assert_eq!(
        (
            view.counts.accepted,
            view.counts.requested,
            view.counts.going,
            view.counts.waitlisted
        ),
        (1, 1, 3, 3)
    );
    let before = social_event::attendees(&uri, None, None, 0, 100).await?;
    test.put(
        &alice,
        &rsvp_path,
        &post("rsvp", &json!({"status":"accepted","guests":2}), Some(&uri)),
    )
    .await?;
    assert_eq!(
        social_event::attendees(&uri, None, None, 0, 100).await?[0].indexed_at,
        before[0].indexed_at
    );
    assert!(Notification::get_by_id(
        &author,
        Pagination {
            limit: Some(100),
            ..Default::default()
        }
    )
    .await?
    .iter()
    .any(|n| matches!(n.body, NotificationBody::Reply { .. })));
    body["start"] = (Utc::now() + Duration::days(3))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
        .into();
    body["locations"] = json!([{"uri":"https://example.com/new-venue"}]);
    body["attendance"]["policy"] = "ticket".into();
    test.put(&owner, &event_path, &post("event", &body, None))
        .await?;
    assert_eq!(
        social_event::event_view(&author, &id, None)
            .await?
            .unwrap()
            .counts
            .going,
        1
    );
    let notifications = Notification::get_by_id(
        &a,
        Pagination {
            limit: Some(100),
            ..Default::default()
        },
    )
    .await?;
    assert!(notifications.iter().any(
        |n| matches!(&n.body,NotificationBody::EventChanged{event_uri,..} if event_uri==&uri)
    ));
    assert!(!Notification::get_by_id(
        &b,
        Pagination {
            limit: Some(100),
            ..Default::default()
        }
    )
    .await?
    .iter()
    .any(|n| matches!(n.body, NotificationBody::EventChanged { .. })));
    let (page, _, _) = ResourceChange::get_from_index(cursor, 1000).await?;
    assert!(page
        .lines
        .iter()
        .all(|l| ![&author, &a, &b].contains(&&l.author)));
    test.del(&alice, &rsvp_path).await?;
    assert_eq!(
        social_event::event_view(&author, &id, None)
            .await?
            .unwrap()
            .counts
            .going,
        0
    );
    test.del(&owner, &event_path).await?;
    assert!(social_event::stream(query(&author)).await?.is_empty());
    assert!(PostDetails::get_by_id(&a, &rsvp_id)
        .await?
        .is_none_or(|p| p.deleted));
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn recurrence_override_delete_restore_orphan_and_weekly_rsvp_cleanup() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let key = Keypair::random();
    let author = test
        .create_user(&key, &test_user("Native series", ""))
        .await?;
    let mut master = content();
    master["recurrenceRules"] = json!([{"frequency":"daily","count":3}]);
    let (id, master_path, uri) = put(&mut test, &key, "event", &master, None).await?;
    let rows = social_event::stream(query(&author)).await?;
    assert_eq!(rows.len(), 3);
    let occurrence = rows[1].recurrence_id.clone();
    let mut replacement = content();
    replacement["recurrenceOf"] = uri.clone().into();
    replacement["recurrenceId"] = occurrence.clone().into();
    replacement["start"] = (Utc::now() + Duration::days(8))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
        .into();
    // Editing a standalone event into an override must remove its old standalone projection.
    let (_, old_path, _) = put(&mut test, &key, "event", &content(), None).await?;
    assert_eq!(social_event::stream(query(&author)).await?.len(), 4);
    test.put(&key, &old_path, &post("event", &replacement, None))
        .await?;
    assert_eq!(social_event::stream(query(&author)).await?.len(), 3);
    replacement["start"] = (Utc::now() + Duration::days(9))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
        .into();
    let (_, new_path, new_uri) = put(&mut test, &key, "event", &replacement, None).await?;
    let rows = social_event::stream(query(&author)).await?;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().any(|o| o.post_uri == new_uri));
    test.del(&key, &new_path).await?;
    assert!(social_event::stream(query(&author))
        .await?
        .iter()
        .any(|o| o.post_uri != uri));
    test.put(&key, &old_path, &post("event", &content(), None))
        .await?;
    assert_eq!(social_event::stream(query(&author)).await?.len(), 4);
    test.put(&key, &old_path, &post("event", &replacement, None))
        .await?;
    assert_eq!(social_event::stream(query(&author)).await?.len(), 3);
    test.del(&key, &old_path).await?;
    assert!(social_event::stream(query(&author))
        .await?
        .iter()
        .all(|o| o.post_uri == uri));
    let attendee = Keypair::random();
    let a = test
        .create_user(&attendee, &test_user("Occurrence attendee", ""))
        .await?;
    put(
        &mut test,
        &attendee,
        "rsvp",
        &json!({"status":"accepted"}),
        Some(&uri),
    )
    .await?;
    let (_, rp, _) = put(
        &mut test,
        &attendee,
        "rsvp",
        &json!({"status":"declined","recurrenceId":occurrence}),
        Some(&uri),
    )
    .await?;
    assert_eq!(
        social_event::event_view(&author, &id, Some(&occurrence))
            .await?
            .unwrap()
            .counts
            .going,
        0
    );
    assert_eq!(
        social_event::event_view(&author, &id, None)
            .await?
            .unwrap()
            .counts
            .going,
        1
    );
    put(
        &mut test,
        &attendee,
        "rsvp",
        &json!({"status":"accepted","guests":1}),
        Some(&uri),
    )
    .await?;
    assert_eq!(
        social_event::event_view(&author, &id, Some(&occurrence))
            .await?
            .unwrap()
            .counts
            .going,
        0
    );
    let (_, orphan_path, orphan_uri) = put(&mut test, &key, "event", &replacement, None).await?;
    assert!(!Notification::get_by_id(&a, Pagination {limit:Some(100), ..Default::default()}).await?
        .iter().any(|n| matches!(&n.body, NotificationBody::EventChanged{recurrence_id,..} if recurrence_id == &occurrence)));
    master["recurrenceRules"] = json!([{"frequency":"daily","count":1}]);
    test.put(&key, &master_path, &post("event", &master, None))
        .await?;
    let rows = social_event::stream(query(&author)).await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|o| o.master == orphan_uri));
    social_event::prune_orphan_rsvps().await?;
    assert_eq!(
        social_event::attendees(&uri, Some(&occurrence), None, 0, 100)
            .await?
            .iter()
            .find(|p| p.author == a)
            .unwrap()
            .status,
        "accepted"
    );
    social_event::maintain_occurrences(Utc::now()).await?;
    test.del(&key, &orphan_path).await?;
    test.del(&attendee, &rp).await?;
    test.del(&key, &master_path).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn ratings_newest_per_author_resource_and_post_aggregates() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let key = Keypair::random();
    let author = test
        .create_user(&key, &test_user("Native ratings", ""))
        .await?;
    let other = Keypair::random();
    test.create_user(&other, &test_user("Other rater", ""))
        .await?;
    let uri = format!("https://example.com/rating/{author}");
    let (_, first_path, _) =
        put(&mut test, &key, "rating", &json!({"rating":2}), Some(&uri)).await?;
    let (_, second_path, _) =
        put(&mut test, &key, "rating", &json!({"rating":8}), Some(&uri)).await?;
    let (_, third_path, _) = put(
        &mut test,
        &other,
        "rating",
        &json!({"rating":10}),
        Some(&uri),
    )
    .await?;
    let stats = social_event::rating_summary(&uri).await?;
    assert_eq!((stats.rating_avg, stats.rating_count), (9.0, 2));
    test.put(
        &key,
        &first_path,
        &post("rating", &json!({"rating":4}), Some(&uri)),
    )
    .await?;
    let stats = social_event::rating_summary(&uri).await?;
    assert_eq!((stats.rating_avg, stats.rating_count), (7.0, 2));
    test.del(&key, &first_path).await?;
    assert_eq!(social_event::rating_summary(&uri).await?.rating_avg, 9.0);
    let (event_id, event_path, event_uri) = put(&mut test, &key, "event", &content(), None).await?;
    let (_, rating_path, _) = put(
        &mut test,
        &other,
        "rating",
        &json!({"rating":7}),
        Some(&event_uri),
    )
    .await?;
    assert_eq!(
        social_event::event_view(&author, &event_id, None)
            .await?
            .unwrap()
            .post
            .rating
            .rating_avg,
        7.0
    );
    test.del(&other, &rating_path).await?;
    assert_eq!(
        social_event::rating_summary(&event_uri).await?.rating_count,
        0
    );
    test.del(&key, &second_path).await?;
    test.del(&other, &third_path).await?;
    assert_eq!(social_event::rating_summary(&uri).await?.rating_count, 0);
    test.del(&key, &event_path).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn override_rsvp_survives_prune_until_its_master_key_is_removed() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Override RSVP", ""))
        .await?;
    let mut master = content();
    master["recurrenceRules"] = json!([{"frequency":"daily","count":2}]);
    let (id, master_path, uri) = put(&mut test, &owner, "event", &master, None).await?;
    let recurrence = social_event::stream(query(&author)).await?[1]
        .recurrence_id
        .clone();
    let mut replacement = content();
    replacement["recurrenceOf"] = uri.clone().into();
    replacement["recurrenceId"] = recurrence.clone().into();
    let (_, override_path, override_uri) =
        put(&mut test, &owner, "event", &replacement, None).await?;
    let (rsvp_id, rsvp_path, _) = put(
        &mut test,
        &owner,
        "rsvp",
        &json!({"status":"accepted","recurrenceId":recurrence}),
        Some(&override_uri),
    )
    .await?;
    for _ in 0..2 {
        social_event::prune_orphan_rsvps().await?;
        let view = social_event::event_view(&author, &id, Some(&recurrence))
            .await?
            .unwrap();
        assert_eq!(view.counts.going, 1);
    }
    master["recurrenceRules"] = json!([{"frequency":"daily","count":1}]);
    test.put(&owner, &master_path, &post("event", &master, None))
        .await?;
    social_event::prune_orphan_rsvps().await?;
    assert!(
        social_event::attendees(&uri, Some(&recurrence), None, 0, 100)
            .await?
            .is_empty()
    );
    assert!(PostDetails::get_by_id(&author, &rsvp_id).await?.is_some());
    test.del(&owner, &rsvp_path).await?;
    test.del(&owner, &override_path).await?;
    test.del(&owner, &master_path).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn plain_notes_skip_projection_and_native_kind_cleanup_can_resume() -> Result<()> {
    use nexus_common::{db::exec_single_row, models::post::PostKind};
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Projection state", ""))
        .await?;
    let note_id = generate_post_id();
    let note_path = path(&note_id);
    for _ in 0..2 {
        test.put(
            &owner,
            &note_path,
            &json!({"kind":"note","content":"plain note"}),
        )
        .await?;
        let row = fetch_row_from_graph(
            Query::new(
                "test_plain_projection",
                r"
            MATCH (p:Post {id:$id}) RETURN [k IN keys(p) WHERE k STARTS WITH 'social_'
                OR k IN ['event_content','all_day','waitlist','guests']] AS projection
        ",
            )
            .param("id", note_id.clone()),
        )
        .await?
        .unwrap();
        assert!(row.get::<Vec<String>>("projection")?.is_empty());
    }
    let body = content();
    let (id, event_path, uri) = put(&mut test, &owner, "event", &body, None).await?;
    let state = || {
        Query::new("test_fingerprint", "MATCH (p:Post {id:$id}) RETURN p.social_fingerprint AS hash,p.social_updated_at AS updated").param("id",id.clone())
    };
    let before = fetch_row_from_graph(state()).await?.unwrap();
    let hash = before.get::<String>("hash")?;
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    test.put(&owner, &event_path, &post("event", &body, None))
        .await?;
    let after = fetch_row_from_graph(state()).await?.unwrap();
    assert_eq!(hash, after.get::<String>("hash")?);
    assert_eq!(before.get::<i64>("updated")?, after.get::<i64>("updated")?);

    // Resume after the graph committed a kind change but before its family cleanup.
    let details = PostDetails {
        author: author.clone(),
        id: id.clone(),
        uri: uri.clone(),
        content: "plain now".into(),
        kind: PostKind::Short,
        indexed_at: Utc::now().timestamp_millis(),
        ..Default::default()
    };
    details.put_to_graph(None, None).await?;
    exec_single_row(
        Query::new(
            "test_partial_kind_cleanup",
            r"
        MATCH (p:Post {id:$id}) SET p.social_pending_roots=[$uri]
        REMOVE p.social_kind,p.event_content
    ",
        )
        .param("id", id.clone())
        .param("uri", uri),
    )
    .await?;
    test.put(
        &owner,
        &event_path,
        &json!({"kind":"note","content":"plain now"}),
    )
    .await?;
    assert!(social_event::stream(query(&author)).await?.is_empty());
    let row = fetch_row_from_graph(
        Query::new(
            "test_projection_cleared",
            r"
        MATCH (p:Post {id:$id}) RETURN [k IN keys(p) WHERE k STARTS WITH 'social_'
            OR k IN ['event_content','all_day','waitlist','guests']] AS projection
    ",
        )
        .param("id", id),
    )
    .await?
    .unwrap();
    assert!(row.get::<Vec<String>>("projection")?.is_empty());
    test.del(&owner, &event_path).await?;
    test.del(&owner, &note_path).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn rating_waits_for_missing_post_then_replay_updates_its_view() -> Result<()> {
    use super::utils::watcher::retrieve_and_handle_event_line;
    use nexus_common::{
        db::exec_single_row,
        models::{
            error::ModelError,
            post::{PostKind, PostView},
        },
    };
    use nexus_watcher::errors::EventProcessorError;
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Delayed rating target", ""))
        .await?;
    let target_id = generate_post_id();
    let target_path = path(&target_id);
    let uri = pubky_social_specs::post_uri_builder(author.clone(), target_id.clone());
    test.ensure_event_processing = false;
    let (_, rating_path, _) = put(
        &mut test,
        &owner,
        "rating",
        &json!({"rating":7}),
        Some(&uri),
    )
    .await?;
    let line = format!("PUT pubky://{author}{rating_path}");
    let handler = test.event_processor_runner.event_handler.clone();
    let err = retrieve_and_handle_event_line(&line, handler.clone())
        .await
        .unwrap_err();
    assert!(
        matches!(err, EventProcessorError::MissingDependency { dependency } if dependency == vec![uri.clone()])
    );

    // A projection retried after an ordinary graph write must also defer, never
    // create a fallback Resource that would retain the rating when the Post arrives.
    let id = generate_post_id();
    let body = json!({"rating":7}).to_string();
    PostDetails {
        author: author.clone(),
        id: id.clone(),
        uri: pubky_social_specs::post_uri_builder(author.clone(), id.clone()),
        content: body.clone(),
        kind: PostKind::Rating,
        indexed_at: Utc::now().timestamp_millis(),
        ..Default::default()
    }
    .put_to_graph(None, None)
    .await?;
    let projection = social_event::SocialContent::read(PostKind::Rating, &body, Utc::now())?;
    let err = social_event::sync(&author, &id, &body, Some(&uri), &projection)
        .await
        .unwrap_err();
    assert!(matches!(&err, ModelError::MissingDependency(target) if target == &uri));
    assert!(
        matches!(EventProcessorError::from(err), EventProcessorError::MissingDependency { dependency } if dependency == vec![uri.clone()])
    );
    let row = fetch_row_from_graph(
        Query::new(
            "test_missing_rating_resource",
            "OPTIONAL MATCH (r:Resource {id:$id}) RETURN count(r) AS n",
        )
        .param(
            "id",
            nexus_common::universal_tag::normalize::resource_id(&uri),
        ),
    )
    .await?
    .unwrap();
    assert_eq!(row.get::<i64>("n")?, 0);
    test.put(
        &owner,
        &target_path,
        &json!({"kind":"note","content":"Target arrives later"}),
    )
    .await?;
    retrieve_and_handle_event_line(
        &format!("PUT pubky://{author}{target_path}"),
        handler.clone(),
    )
    .await?;
    for _ in 0..2 {
        social_event::sync(&author, &id, &body, Some(&uri), &projection).await?;
        retrieve_and_handle_event_line(&line, handler.clone()).await?;
        let view = PostView::get_by_id(&author, &target_id, None, None, None)
            .await?
            .unwrap();
        assert_eq!((view.rating.rating_avg, view.rating.rating_count), (7.0, 1));
    }
    social_event::remove(&author, &id).await?;
    exec_single_row(
        Query::new(
            "test_rating_cleanup",
            "MATCH (p:Post {id:$id}) DETACH DELETE p",
        )
        .param("id", id),
    )
    .await?;
    test.ensure_event_processing = true;
    test.del(&owner, &rating_path).await?;
    test.del(&owner, &target_path).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn projection_replay_recovers_graph_commit_and_notification_outbox() -> Result<()> {
    use deadpool_redis::redis::AsyncCommands;
    use nexus_common::models::post::PostKind;
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Recovery owner", ""))
        .await?;
    let attendee = Keypair::random();
    let a = test
        .create_user(&attendee, &test_user("Recovery attendee", ""))
        .await?;
    let id = generate_post_id();
    let event_path = path(&id);
    let uri = pubky_social_specs::post_uri_builder(author.clone(), id.clone());
    let mut body = content();
    let details = PostDetails {
        author: author.clone(),
        id: id.clone(),
        uri: uri.clone(),
        content: body.to_string(),
        kind: PostKind::Event,
        indexed_at: Utc::now().timestamp_millis(),
        ..Default::default()
    };
    details.put_to_graph(None, None).await?; // graph committed, process died before Redis/projection
    test.put(&owner, &event_path, &post("event", &body, None))
        .await?;
    assert_eq!(social_event::stream(query(&author)).await?.len(), 1);
    put(
        &mut test,
        &attendee,
        "rsvp",
        &json!({"status":"accepted"}),
        Some(&uri),
    )
    .await?;
    body["status"] = "cancelled".into();
    let revised = PostDetails {
        content: body.to_string(),
        ..details
    };
    revised.put_to_graph(None, None).await?;
    let notification_key = format!("Sorted:Notification:{a}");
    let mut redis = nexus_common::db::get_redis_conn().await?;
    let _: () = redis
        .set(&notification_key, "simulated notification write failure")
        .await?;
    let projection =
        social_event::SocialContent::read(PostKind::Event, &body.to_string(), Utc::now())?;
    let result = social_event::sync(&author, &id, &body.to_string(), None, &projection).await;
    let _: () = redis.del(&notification_key).await?; // restore before any assertion
    assert!(result.is_err());
    let pending = fetch_row_from_graph(
        Query::new(
            "test_event_notice_pending",
            "MATCH (n:EventNotice {user_id:$a}) RETURN count(n) AS n",
        )
        .param("a", a.clone()),
    )
    .await?
    .unwrap();
    assert_eq!(pending.get::<i64>("n")?, 1);
    test.put(&owner, &event_path, &post("event", &body, None))
        .await?;
    test.put(&owner, &event_path, &post("event", &body, None))
        .await?;
    let notices = Notification::get_by_id(
        &a,
        Pagination {
            limit: Some(100),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        notices
            .iter()
            .filter(|n| matches!(n.body, NotificationBody::EventChanged { .. }))
            .count(),
        1
    );
    let pending = fetch_row_from_graph(
        Query::new(
            "test_event_notice_done",
            "MATCH (n:EventNotice {user_id:$a}) RETURN count(n) AS n",
        )
        .param("a", a),
    )
    .await?
    .unwrap();
    assert_eq!(pending.get::<i64>("n")?, 0);
    test.del(&owner, &event_path).await?;
    Ok(())
}

#[tokio_shared_rt::test(shared)]
async fn invalid_native_payloads_and_non_event_rsvp_parent_are_dropped() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Validation owner", ""))
        .await?;
    let note_id = generate_post_id();
    let note_path = path(&note_id);
    test.put(
        &owner,
        &note_path,
        &json!({"kind":"note","content":"ordinary"}),
    )
    .await?;
    let note_uri = pubky_social_specs::post_uri_builder(author.clone(), note_id);
    let mut far = content();
    far["start"] = "2099-01-01T12:00:00".into();
    let mut zone = content();
    zone["timeZone"] = "No/SuchZone".into();
    let mut rule = content();
    rule["recurrenceRules"] = json!([{"frequency":"daily","count":1001}]);
    for (kind, body, parent) in [
        ("event", far, None),
        ("event", zone, None),
        ("event", rule, None),
        ("rating", json!({"rating":0}), Some(note_uri.as_str())),
        ("rating", json!({"rating":11}), Some(note_uri.as_str())),
        (
            "rsvp",
            json!({"status":"accepted"}),
            Some(note_uri.as_str()),
        ),
    ] {
        let id = generate_post_id();
        test.put(&owner, &path(&id), &post(kind, &body, parent))
            .await?;
        assert!(
            PostDetails::get_by_id(&author, &id).await?.is_none(),
            "invalid {kind} was indexed"
        );
    }
    test.del(&owner, &note_path).await?;
    Ok(())
}

// The nextest override runs this alone: it advances the shared occurrence window, then restores it.
#[tokio_shared_rt::test(shared)]
async fn daily_window_rolls_forward_without_pruning_future_rsvps() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let owner = Keypair::random();
    let author = test
        .create_user(&owner, &test_user("Rolling calendar", ""))
        .await?;
    let now = Utc::now();
    let start = now.checked_add_months(chrono::Months::new(13)).unwrap();
    let mut body = content();
    body["start"] = start.format("%Y-%m-%dT%H:%M:%S").to_string().into();
    let (id, event_path, uri) = put(&mut test, &owner, "event", &body, None).await?;
    let (_, rsvp_path, _) = put(
        &mut test,
        &owner,
        "rsvp",
        &json!({"status":"accepted","recurrenceId":body["start"]}),
        Some(&uri),
    )
    .await?;
    assert!(social_event::event_view(&author, &id, None)
        .await?
        .unwrap()
        .occurrences
        .is_empty());
    social_event::prune_orphan_rsvps().await?;
    assert_eq!(
        social_event::event_view(&author, &id, body["start"].as_str())
            .await?
            .unwrap()
            .counts
            .going,
        1
    );
    let later = now.checked_add_months(chrono::Months::new(2)).unwrap();
    social_event::maintain_occurrences(later).await?;
    let first = social_event::event_view(&author, &id, None)
        .await?
        .unwrap()
        .occurrences;
    social_event::maintain_occurrences(later).await?;
    let second = social_event::event_view(&author, &id, None)
        .await?
        .unwrap()
        .occurrences;
    // Restore even before assertions, so a failing assertion cannot leave the shifted window.
    social_event::maintain_occurrences(now).await?;
    assert_eq!(first.len(), 1);
    assert_eq!(first, second);
    assert!(social_event::event_view(&author, &id, None)
        .await?
        .unwrap()
        .occurrences
        .is_empty());
    test.del(&owner, &rsvp_path).await?;
    test.del(&owner, &event_path).await?;
    Ok(())
}
