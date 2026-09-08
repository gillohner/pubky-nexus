//! Graph-backed reads for the private source replication protocol.
use super::{queries, types::*};
use crate::db::fetch_row_from_graph;
use serde::Deserialize;

pub async fn head() -> Result<Checkpoint, ProjectionError> {
    let row = required_row(queries::head()).await?;
    row.get("checkpoint")
        .map_err(crate::db::GraphError::from)
        .map_err(Into::into)
}

pub struct InventoryRequest<'a> {
    pub epoch: &'a str,
    pub since: i64,
    pub after: Option<&'a str>,
    pub kinds: &'a [String],
    pub limit: usize,
}

#[derive(Deserialize)]
pub(super) struct InventoryRow {
    pub(super) key: String,
    pub(super) item: SourceChange,
}

pub async fn inventory(request: InventoryRequest<'_>) -> Result<InventoryPage, ProjectionError> {
    validate_limit(request.limit)?;
    let row = required_row(queries::inventory(
        request.after,
        request.kinds,
        request.limit,
    ))
    .await?;
    let checkpoint: Checkpoint = row.get("checkpoint").map_err(crate::db::GraphError::from)?;
    checkpoint.validate(request.epoch, request.since)?;
    let mut rows: Vec<InventoryRow> = row.get("rows").map_err(crate::db::GraphError::from)?;
    let has_more = rows.len() > request.limit;
    rows.truncate(request.limit);
    let next_cursor = has_more
        .then(|| rows.last().map(|row| row.key.clone()))
        .flatten();
    Ok(InventoryPage {
        checkpoint,
        items: rows.into_iter().map(|row| row.item).collect(),
        next_cursor,
    })
}

pub struct ChangesRequest<'a> {
    pub epoch: &'a str,
    pub after: i64,
    pub through: Option<i64>,
    pub limit: usize,
}

pub async fn changes(request: ChangesRequest<'_>) -> Result<ChangesPage, ProjectionError> {
    validate_limit(request.limit)?;
    if request
        .through
        .is_some_and(|through| through < request.after)
    {
        return Err(ProjectionError::InvalidInput(
            "through must not precede after",
        ));
    }
    let row = required_row(queries::changes(
        request.after,
        request.through,
        request.limit,
    ))
    .await?;
    let checkpoint: Checkpoint = row.get("checkpoint").map_err(crate::db::GraphError::from)?;
    checkpoint.validate(request.epoch, request.after)?;
    let through: String = row.get("through").map_err(crate::db::GraphError::from)?;
    checkpoint.validate(request.epoch, parse_revision(&through)?)?;
    let mut items: Vec<SourceChange> = row.get("items").map_err(crate::db::GraphError::from)?;
    let caught_up = items.len() <= request.limit;
    items.truncate(request.limit);
    let cursor = if caught_up {
        through.clone()
    } else {
        items
            .last()
            .map(|item| item.revision.clone())
            .unwrap_or_else(|| request.after.to_string())
    };
    Ok(ChangesPage {
        checkpoint,
        items,
        cursor,
        through,
        caught_up,
    })
}

async fn required_row(query: crate::db::graph::Query) -> Result<neo4rs::Row, ProjectionError> {
    fetch_row_from_graph(query).await?.ok_or_else(|| {
        ProjectionError::Graph(crate::db::GraphError::Generic(
            "Projection checkpoint missing".into(),
        ))
    })
}

fn validate_limit(limit: usize) -> Result<(), ProjectionError> {
    if !(1..=100).contains(&limit) {
        return Err(ProjectionError::InvalidInput("limit must be 1–100"));
    }
    Ok(())
}
