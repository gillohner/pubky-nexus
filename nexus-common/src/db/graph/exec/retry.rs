//! Retry only transactions Neo4j explicitly aborted because of a deadlock.
//!
//! neo4rs 0.8 retries RUN failures, but not failures received while consuming
//! PULL results. Drain each mutation attempt before exposing its outcome so a
//! later rollback cannot be mistaken for a committed graph/cache boundary.

use std::time::Duration;

use futures::TryStreamExt;
use neo4rs::{Error, Row};

use crate::db::graph::{GraphOps, Query};

const DEADLOCK_CODE: &str = "Neo.TransientError.Transaction.DeadlockDetected";
const MAX_RETRIES: u32 = 3;

pub(super) async fn fetch_mutation_row(
    graph: &dyn GraphOps,
    query: Query,
) -> neo4rs::Result<Option<Row>> {
    for attempt in 0..=MAX_RETRIES {
        match execute_and_drain(graph, query.clone()).await {
            Err(error) if is_deadlock(&error) && attempt < MAX_RETRIES => {
                tracing::warn!(
                    query = query.label(),
                    attempt,
                    "Retrying aborted Neo4j mutation"
                );
                tokio::time::sleep(Duration::from_millis(50 * (1 << attempt))).await;
            }
            result => return result,
        }
    }
    unreachable!("the final attempt always returns its result")
}

async fn execute_and_drain(graph: &dyn GraphOps, query: Query) -> neo4rs::Result<Option<Row>> {
    let mut rows = graph.execute(query).await?;
    let first = rows.try_next().await?;
    while rows.try_next().await?.is_some() {}
    Ok(first)
}

fn is_deadlock(error: &Error) -> bool {
    match error {
        Error::Neo4j(error) => error.code() == DEADLOCK_CODE,
        // neo4rs 0.8's PULL decoder exposes Failure metadata only as Debug text.
        // Match the code field, never the server message or arbitrary I/O text.
        Error::UnexpectedMessage(message) => {
            message.starts_with("unexpected response for PULL: Ok(Failure(")
                && message.contains(concat!(
                    "BoltString { value: \"code\" }: String(BoltString { value: ",
                    "\"Neo.TransientError.Transaction.DeadlockDetected\" })"
                ))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
