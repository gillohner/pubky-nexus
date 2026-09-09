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
// Eight concurrent checkpoint writers can require more than three aborted attempts.
// Keep retries bounded and desynchronize contenders; only confirmed rollbacks qualify.
const MAX_RETRIES: u32 = 8;
const MAX_BACKOFF_MS: u64 = 1_000;

fn retry_delay(attempt: u32) -> Duration {
    let ceiling = (50_u64 << attempt.min(5)).min(MAX_BACKOFF_MS);
    Duration::from_millis(rand::random_range(ceiling / 2..=ceiling))
}

pub(super) async fn fetch_mutation_row(
    graph: &dyn GraphOps,
    query: Query,
) -> neo4rs::Result<Option<Row>> {
    for attempt in 0..=MAX_RETRIES {
        match execute_and_drain(graph, query.clone()).await {
            Err(error) if is_deadlock(&error) && attempt < MAX_RETRIES => {
                let delay = retry_delay(attempt);
                tracing::warn!(
                    query = query.label(),
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    "Retrying aborted Neo4j mutation"
                );
                tokio::time::sleep(delay).await;
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
