use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use futures::{stream, stream::BoxStream, StreamExt};
use neo4rs::{BoltType, Error, Row};

use super::{fetch_mutation_row, is_deadlock, GraphOps, Query, MAX_RETRIES};

struct ScriptedGraph {
    attempts: Mutex<VecDeque<neo4rs::Result<Vec<neo4rs::Result<Row>>>>>,
    queries: Mutex<Vec<String>>,
}

impl ScriptedGraph {
    fn new(attempts: Vec<neo4rs::Result<Vec<neo4rs::Result<Row>>>>) -> Self {
        Self {
            attempts: Mutex::new(attempts.into()),
            queries: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl GraphOps for ScriptedGraph {
    async fn execute(
        &self,
        query: Query,
    ) -> neo4rs::Result<BoxStream<'static, neo4rs::Result<Row>>> {
        self.queries
            .lock()
            .unwrap()
            .push(query.to_cypher_populated());
        let rows = self.attempts.lock().unwrap().pop_front().unwrap()?;
        Ok(stream::iter(rows).boxed())
    }

    async fn run(&self, _query: Query) -> neo4rs::Result<()> {
        panic!("mutation outcomes must consume the result stream")
    }
}

fn deadlock() -> Error {
    Error::UnexpectedMessage(
        concat!(
            "unexpected response for PULL: Ok(Failure(Failure { metadata: BoltMap { value: {",
            "BoltString { value: \"code\" }: String(BoltString { value: ",
            "\"Neo.TransientError.Transaction.DeadlockDetected\" }), ",
            "BoltString { value: \"message\" }: String(BoltString { value: \"lock cycle\" })",
            "} } }))"
        )
        .into(),
    )
}

fn row(flag: bool) -> Row {
    Row::new(
        vec![BoltType::from("flag")].into(),
        vec![BoltType::from(flag)].into(),
    )
}

fn query() -> Query {
    Query::new("test_mutation", "RETURN $flag AS flag").param("flag", true)
}

#[tokio::test]
async fn pull_deadlock_discards_partial_rows_and_retries_the_same_query() {
    let graph = ScriptedGraph::new(vec![
        Ok(vec![Ok(row(false)), Err(deadlock())]),
        Ok(vec![Ok(row(true))]),
    ]);
    let result = fetch_mutation_row(&graph, query()).await.unwrap().unwrap();
    assert!(result.get::<bool>("flag").unwrap());
    assert_eq!(
        *graph.queries.lock().unwrap(),
        ["RETURN true AS flag", "RETURN true AS flag"]
    );
}

#[tokio::test]
async fn repeated_deadlocks_stop_after_the_bounded_attempts() {
    let attempts = (0..=MAX_RETRIES)
        .map(|_| Ok(vec![Err(deadlock())]))
        .collect();
    let graph = ScriptedGraph::new(attempts);
    let error = fetch_mutation_row(&graph, query()).await.unwrap_err();
    assert!(is_deadlock(&error));
    assert_eq!(
        graph.queries.lock().unwrap().len(),
        (MAX_RETRIES + 1) as usize
    );
}

#[tokio::test]
async fn ambiguous_connection_failure_is_returned_without_retry() {
    let graph = ScriptedGraph::new(vec![Ok(vec![Ok(row(true)), Err(Error::ConnectionError)])]);
    assert!(matches!(
        fetch_mutation_row(&graph, query()).await,
        Err(Error::ConnectionError)
    ));
    assert_eq!(graph.queries.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn execute_failure_is_returned_without_retry() {
    let graph = ScriptedGraph::new(vec![Err(Error::ConnectionError)]);
    assert!(matches!(
        fetch_mutation_row(&graph, query()).await,
        Err(Error::ConnectionError)
    ));
    assert_eq!(graph.queries.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn empty_mutation_result_keeps_missing_dependency_semantics() {
    let graph = ScriptedGraph::new(vec![Ok(vec![])]);
    assert!(fetch_mutation_row(&graph, query()).await.unwrap().is_none());
    assert_eq!(graph.queries.lock().unwrap().len(), 1);
}

#[test]
fn deadlock_text_in_another_error_field_does_not_enable_retry() {
    let Error::UnexpectedMessage(message) = deadlock() else {
        unreachable!()
    };
    assert!(!is_deadlock(&Error::UnexpectedMessage(
        message.replace("value: \"code\"", "value: \"message\"")
    )));
    assert!(!is_deadlock(&Error::UnexpectedMessage(
        message.replace("for PULL", "for RESET")
    )));
}
