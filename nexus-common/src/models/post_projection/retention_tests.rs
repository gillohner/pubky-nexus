//! Retention fixtures use private checkpoint IDs and roll back every graph write.
use super::{mutation::prune_history, Checkpoint};
use neo4rs::{query, Graph};

#[test]
fn a_reset_floor_rejects_unavailable_history_in_the_new_epoch() {
    let reset = Checkpoint {
        epoch: "new-generation".into(),
        revision: "12001".into(),
        minimum_revision: "12000".into(),
    };
    assert!(reset.validate("new-generation", 12000).is_ok());
    assert!(reset.validate("new-generation", 12001).is_ok());
    assert!(reset.validate("new-generation", 11999).is_err());
    assert!(reset.validate("old-generation", 12000).is_err());
}

#[tokio::test]
#[ignore = "requires NEXUS_PROJECTION_TEST_URI=127.0.0.1:17687; all writes roll back"]
async fn retention_floor_is_monotonic_in_rollback_only_fixtures() {
    let uri = std::env::var("NEXUS_PROJECTION_TEST_URI").expect("explicit isolated Neo4j URI");
    assert_eq!(uri, "127.0.0.1:17687", "refuse a shared or remote database");
    let graph = Graph::new(uri, "neo4j", "unused").await.unwrap();
    let fixture = format!("retention-test-{}", pubky::Keypair::random().public_key());
    let mut transaction = graph.start_txn().await.unwrap();
    for (name, revision, minimum, expected) in [
        ("initial", 1_i64, 0_i64, 0_i64),
        ("reset", 12001, 12000, 12000),
        ("advance", 22001, 12000, 12001),
    ] {
        let epoch = format!("{fixture}-{name}");
        let cypher = format!(
            "CREATE (checkpoint:PostProjectionCheckpoint {{id:$epoch, epoch:$epoch,
                revision:$revision, minimum_revision:$minimum, test_fixture:$fixture}})
             CREATE (:PostSourceChange {{epoch:$epoch, revision:$expected, test_fixture:$fixture}})
             CREATE (:PostSourceChange {{epoch:$epoch, revision:$expected + 1, test_fixture:$fixture}})
             WITH checkpoint
             {}
             RETURN checkpoint.minimum_revision AS minimum",
            prune_history()
        );
        let mut rows = transaction
            .execute(
                query(&cypher)
                    .param("epoch", epoch.clone())
                    .param("fixture", fixture.clone())
                    .param("revision", revision)
                    .param("minimum", minimum)
                    .param("expected", expected),
            )
            .await
            .unwrap();
        let row = rows.next(transaction.handle()).await.unwrap().unwrap();
        assert!(rows.next(transaction.handle()).await.unwrap().is_none());
        assert_eq!(row.get::<i64>("minimum").unwrap(), expected, "{name}");

        let mut retained = transaction.execute(query(
            "MATCH (change:PostSourceChange {epoch:$epoch}) RETURN change.revision AS revision"
        ).param("epoch", epoch)).await.unwrap();
        let row = retained.next(transaction.handle()).await.unwrap().unwrap();
        assert_eq!(row.get::<i64>("revision").unwrap(), expected + 1, "{name}");
        assert!(retained.next(transaction.handle()).await.unwrap().is_none());
    }
    transaction.rollback().await.unwrap();
    let mut rows = graph
        .execute(
            query("MATCH (n {test_fixture:$fixture}) RETURN count(n) AS count")
                .param("fixture", fixture),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert!(rows.next().await.unwrap().is_none());
    assert_eq!(
        row.get::<i64>("count").unwrap(),
        0,
        "all fixture writes must roll back"
    );
}
