//! Opt-in integration test against an empty, disposable local Neo4j database.
//! Run with NEXUS_PROJECTION_TEST_URI=127.0.0.1:17687 and --ignored.

use super::{queries, Checkpoint, InventoryRow, SourceChange};
use crate::db::graph::{queries::{del, put}, Query};
use crate::models::post::{PostDetails, PostKind, PostRelationships};
use neo4rs::{query, Graph, Row};
use pubky_app_specs::ParsedUri;

const AUTHOR: &str = "4snwyct86m383rsduhw5xgcxpw7c63j3pq8x4ycqikxgik8y64ro";

fn post(id: &str, kind: &str, content: &str) -> PostDetails {
    PostDetails {
        id: id.into(), author: AUTHOR.into(), kind: serde_json::from_value(serde_json::json!(kind)).unwrap(),
        content: content.into(), indexed_at: 1,
        uri: format!("pubky://{AUTHOR}/pub/pubky.app/posts/{id}"),
        attachments: Some(vec![format!("pubky://{AUTHOR}/pub/pubky.app/files/003286NSMY490")]),
        embed: Some("https://example.org/?q=calendar".into()), lock: Some("pubky://lock.example/pub/lock".into()),
    }
}

async fn row(graph: &Graph, source: Query) -> Row {
    let mut rows = graph.execute(source.into()).await.unwrap();
    let result = rows.next().await.unwrap().expect("query must return a row");
    // Consume the stream so the implicit write transaction commits before the next request.
    assert!(rows.next().await.unwrap().is_none());
    result
}

async fn checkpoint(graph: &Graph) -> Checkpoint {
    row(graph, queries::head()).await.get("checkpoint").unwrap()
}

#[tokio::test]
#[ignore = "requires an empty disposable Neo4j on localhost:17687"]
async fn source_mutations_replay_atomically_under_concurrency() {
    let uri = std::env::var("NEXUS_PROJECTION_TEST_URI").expect("set the disposable Neo4j URI");
    assert_eq!(uri, "127.0.0.1:17687", "refuse any shared or remote database");
    let graph = Graph::new(uri, "neo4j", "unused").await.unwrap();
    let count: i64 = row(&graph, Query::new("test_empty", "MATCH (n) RETURN count(n) AS count")).await.get("count").unwrap();
    assert_eq!(count, 0, "start with an empty disposable database; this test never deletes pre-existing data");
    for cypher in [
        "CREATE CONSTRAINT projection_test_post IF NOT EXISTS FOR (p:Post) REQUIRE p.id IS UNIQUE",
        "CREATE CONSTRAINT projection_test_checkpoint IF NOT EXISTS FOR (c:PostProjectionCheckpoint) REQUIRE c.id IS UNIQUE",
        "CREATE CONSTRAINT projection_test_change IF NOT EXISTS FOR (c:PostSourceChange) REQUIRE (c.epoch, c.revision) IS UNIQUE",
    ] { graph.run(query(cypher)).await.unwrap(); }
    graph.run(query("CREATE (:User {id: $author})").param("author", AUTHOR)).await.unwrap();
    let initial = checkpoint(&graph).await;
    assert_eq!(initial.revision, "0");

    let parent = post("003286NSMY490", "calendar", "calendar source");
    let event = post("003286NSMY491", "event", "original event 'quoted'\n$kind");
    let relation = PostRelationships { replied: Some(ParsedUri::try_from(parent.uri.as_str()).unwrap()), ..Default::default() };
    assert!(!row(&graph, put::create_post(&parent, &PostRelationships::default()).unwrap()).await.get::<bool>("flag").unwrap());
    assert!(!row(&graph, put::create_post(&event, &relation).unwrap()).await.get::<bool>("flag").unwrap());
    let inventory = row(&graph, queries::inventory(None, &["event".into(), "calendar".into()], 100)).await;
    let inventory_items: Vec<InventoryRow> = inventory.get("rows").unwrap();
    assert_eq!(inventory_items.len(), 2);
    let original = inventory_items[1].item.post.as_ref().unwrap();
    assert_eq!(original.content, event.content);
    assert_eq!(original.parent.as_deref(), Some(parent.uri.as_str()));
    assert_eq!(original.embed, event.embed);
    assert_eq!(original.attachments, event.attachments);
    assert_eq!(original.lock, event.lock);
    let next = row(&graph, queries::inventory(Some(&parent.id), &["event".into()], 100)).await;
    assert_eq!(next.get::<Vec<InventoryRow>>("rows").unwrap().len(), 1);

    // A transaction rollback must roll back both the source and its revision/payload.
    let before = checkpoint(&graph).await;
    let mut transaction = graph.start_txn().await.unwrap();
    let rollback = post("003286NSMY492", "event", "must never be visible");
    transaction.run(put::create_post(&rollback, &PostRelationships::default()).unwrap().into()).await.unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(checkpoint(&graph).await.revision, before.revision);

    // Concurrent source writes serialize on the checkpoint and never duplicate revisions.
    let futures = (0..8).map(|index| {
        let graph = graph.clone();
        async move {
            let concurrent = post(&format!("003286NSMY5{index:02}"), "event", &format!("concurrent {index}"));
            row(&graph, put::create_post(&concurrent, &PostRelationships::default()).unwrap()).await;
        }
    });
    futures::future::join_all(futures).await;
    assert_eq!(checkpoint(&graph).await.revision, "10");

    let changed_kind = PostDetails { kind: PostKind::Short, content: "now ordinary".into(), ..event.clone() };
    assert!(row(&graph, put::create_post(&changed_kind, &relation).unwrap()).await.get::<bool>("flag").unwrap());
    graph.run(del::delete_post(AUTHOR, &event.id).into()).await.unwrap();
    let replay = row(&graph, queries::changes(0, Some(12), 100)).await;
    let changes: Vec<SourceChange> = replay.get("items").unwrap();
    assert_eq!(changes.len(), 12);
    for (index, change) in changes.iter().enumerate() { assert_eq!(change.revision, (index + 1).to_string()); }
    assert_eq!(changes[1].post.as_ref().unwrap().content, event.content, "history is immutable after edit and delete");
    assert_eq!(changes[10].post.as_ref().unwrap().kind, "short");
    assert_eq!(changes[11].uri, event.uri);
    assert!(changes[11].post.is_none(), "delete has a durable null source");
    let inventory = row(&graph, queries::inventory(None, &["event".into()], 100)).await;
    assert_eq!(inventory.get::<Vec<InventoryRow>>("rows").unwrap().len(), 8);

    // User removal cannot orphan authored sources during a concurrent deletion sweep.
    graph.run(del::delete_user(AUTHOR).into()).await.unwrap();
    let count: i64 = row(&graph, Query::new("test_author_retained", "MATCH (:User)-[:AUTHORED]->(:Post) RETURN count(*) AS count")).await.get("count").unwrap();
    assert_eq!(count, 9);

    // Force a retention boundary without issuing ten thousand source writes.
    graph.run(query("MATCH (c:PostProjectionCheckpoint) SET c.revision = 10001")).await.unwrap();
    row(&graph, put::create_post(&parent, &PostRelationships::default()).unwrap()).await;
    let retained = checkpoint(&graph).await;
    assert_eq!(retained.minimum_revision, "2");
    assert!(retained.validate(&initial.epoch, 1).is_err());
    assert!(retained.validate(&initial.epoch, 2).is_ok());
    let remaining: Vec<SourceChange> = row(&graph, queries::changes(0, None, 100)).await.get("items").unwrap();
    assert!(remaining.iter().all(|change| change.revision.parse::<i64>().unwrap() > 2));
}
