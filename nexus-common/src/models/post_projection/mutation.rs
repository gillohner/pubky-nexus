//! Cypher fragments shared by every indexed Post source mutation.
//!
//! The dependent SET obtains the singleton write lock before any Post locks.
//! Counter, source mutation, immutable payload and retention watermark commit together.

/// Bound retained payload history by revision count. Cursor expiry is explicit.
/// At the custom-envelope maximum this can consume roughly 5 GiB; size the graph accordingly.
pub const RETAINED_REVISIONS: i64 = 10_000;

pub const LOCK_CHECKPOINT: &str = "
    MERGE (checkpoint:PostProjectionCheckpoint {id: 'posts'})
    ON CREATE SET checkpoint.epoch = randomUUID(), checkpoint.revision = 0,
                  checkpoint.minimum_revision = 0
    SET checkpoint._read_lock = true
    REMOVE checkpoint._read_lock
    WITH checkpoint
";

pub const BEGIN_MUTATION: &str = "
    MERGE (checkpoint:PostProjectionCheckpoint {id: 'posts'})
    ON CREATE SET checkpoint.epoch = randomUUID(), checkpoint.revision = 0,
                  checkpoint.minimum_revision = 0
    SET checkpoint.revision = checkpoint.revision + 1
    WITH checkpoint
";

pub const RECORD_PUT: &str = "
    SET new_post.projection_revision = checkpoint.revision,
        new_post.source_parent = $source_parent
    CREATE (change:PostSourceChange {
        epoch: checkpoint.epoch, revision: checkpoint.revision,
        uri: $source_uri, kind: new_post.kind, content: new_post.content,
        attachments: new_post.attachments
    })
    SET change.parent = $source_parent, change.embed = new_post.embed,
        change.lock = new_post.lock
";

pub const RECORD_DELETE: &str = "
    CREATE (:PostSourceChange {
        epoch: checkpoint.epoch, revision: checkpoint.revision,
        uri: 'pubky://' + $author_id + '/pub/pubky.app/posts/' + $post_id,
        deleted: true
    })
";

/// Preserve any higher floor established by an administrative epoch reset.
pub fn prune_history() -> String {
    format!(
        "
        SET checkpoint.minimum_revision = CASE
            WHEN checkpoint.revision - {RETAINED_REVISIONS} > checkpoint.minimum_revision
            THEN checkpoint.revision - {RETAINED_REVISIONS}
            ELSE checkpoint.minimum_revision END
        WITH *
        CALL {{
            WITH checkpoint
            MATCH (expired:PostSourceChange)
            WHERE expired.epoch = checkpoint.epoch
              AND expired.revision <= checkpoint.minimum_revision
            DELETE expired
        }}
    "
    )
}
