use super::mutation::LOCK_CHECKPOINT;
use crate::db::graph::Query;

const CHECKPOINT_MAP: &str = "{epoch: checkpoint.epoch, revision: toString(checkpoint.revision), minimum_revision: toString(checkpoint.minimum_revision)}";

pub fn head() -> Query {
    Query::new(
        "post_projection_head",
        format!("{LOCK_CHECKPOINT} RETURN {CHECKPOINT_MAP} AS checkpoint"),
    )
}

pub fn inventory(after: Option<&str>, kinds: &[String], limit: usize) -> Query {
    Query::new("post_projection_inventory", format!("
        {LOCK_CHECKPOINT}
        CALL {{
            MATCH (p:Post)<-[:AUTHORED]-(author:User)
            WHERE ($after IS NULL OR p.id > $after)
              AND (size($kinds) = 0 OR p.kind IN $kinds)
            WITH p, author ORDER BY p.id LIMIT $limit
            OPTIONAL MATCH (p)-[:REPLIED]->(parent:Post)<-[:AUTHORED]-(parent_author:User)
            WITH p, author, head(collect(CASE WHEN parent IS NULL THEN null
                 ELSE 'pubky://' + parent_author.id + '/pub/pubky.app/posts/' + parent.id END)) AS legacy_parent
            WITH p.id AS key, {{
                revision: toString(coalesce(p.projection_revision, 0)),
                uri: 'pubky://' + author.id + '/pub/pubky.app/posts/' + p.id,
                post: {{kind: p.kind, content: p.content,
                       parent: CASE WHEN p.projection_revision IS NULL THEN legacy_parent ELSE p.source_parent END,
                       embed: p.embed, attachments: p.attachments, lock: p.lock}}
            }} AS item
            ORDER BY key
            RETURN collect({{key: key, item: item}}) AS rows
        }}
        RETURN {CHECKPOINT_MAP} AS checkpoint, rows
    "))
    .param("after", after.map(str::to_owned))
    .param("kinds", kinds.to_vec())
    .param("limit", (limit + 1) as i64)
}

pub fn changes(after: i64, through: Option<i64>, limit: usize) -> Query {
    Query::new(
        "post_projection_changes",
        format!(
            "
        {LOCK_CHECKPOINT}
        WITH checkpoint, coalesce($through, checkpoint.revision) AS through
        CALL {{
            WITH checkpoint, through
            MATCH (change:PostSourceChange)
            WHERE change.epoch = checkpoint.epoch
              AND change.revision > $after AND change.revision <= through
            WITH change ORDER BY change.revision LIMIT $limit
            RETURN collect({{
                revision: toString(change.revision), uri: change.uri,
                post: CASE WHEN change.deleted THEN null ELSE {{
                    kind: change.kind, content: change.content, parent: change.parent,
                    embed: change.embed, attachments: change.attachments, lock: change.lock
                }} END
            }}) AS items
        }}
        RETURN {CHECKPOINT_MAP} AS checkpoint, toString(through) AS through, items
    "
        ),
    )
    .param("after", after)
    .param("through", through)
    .param("limit", (limit + 1) as i64)
}
