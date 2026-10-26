use super::resource_changes::ResourceChangeOp;
use super::resource_changes::{epoch_seed, pending_key, EPOCH_KEY, RESOURCE_CHANGES_KEY};
use crate::db::get_redis_conn;
use crate::db::kv::{RedisError, RedisResult};
use crate::models::app_post::{AppPost, AppPostUri};
use deadpool_redis::redis::{AsyncCommands, Script};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use utoipa::ToSchema;

/// Current feed_only references, retained only to publish deletes and rebuild
/// their feed snapshot. No content or extensions are stored here.
pub const FEED_ONLY_APP_POSTS_KEY: &str = "Event:ResourceChanges:FeedOnlyAppPosts";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AppPostChange {
    pub op: ResourceChangeOp,
    pub author: String,
    /// Versionless logical post path. The replay key is (author, path).
    pub path: String,
    pub namespace: String,
    /// App-owned kind token, not a resource-reference kind.
    pub kind: String,
}

impl AppPostChange {
    pub fn new(post: &AppPostUri, kind: String, op: ResourceChangeOp) -> Self {
        Self {
            op,
            author: post.author.to_string(),
            path: post.path(),
            namespace: post.namespace.clone(),
            kind,
        }
    }

    pub fn from_post(post: AppPost, op: ResourceChangeOp) -> Self {
        Self {
            op,
            author: post.author,
            path: post.path,
            namespace: post.namespace,
            kind: post.kind,
        }
    }

    fn json(&self) -> RedisResult<String> {
        serde_json::to_string(self).map_err(|e| RedisError::SerializationFailed(Box::new(e)))
    }

    pub fn uri(&self) -> String {
        format!("pubky://{}{}", self.author, self.path)
    }

    /// Call after the graph commit (or immediately for feed_only kinds). Append
    /// and feed_only metadata maintenance share the feed's atomic append order.
    pub async fn store(&self, feed_only: bool) -> RedisResult<()> {
        static APPEND: LazyLock<Script> = LazyLock::new(|| {
            Script::new(
                r"
            redis.call('SETNX', KEYS[2], ARGV[2])
            redis.call('RPUSH', KEYS[1], ARGV[1])
            redis.call('DEL', KEYS[3])
            if ARGV[4] == '1' then
                redis.call('HSET', KEYS[4], ARGV[3], ARGV[1])
            else
                redis.call('HDEL', KEYS[4], ARGV[3])
            end
        ",
            )
        });
        APPEND
            .key(RESOURCE_CHANGES_KEY)
            .key(EPOCH_KEY)
            .key(pending_key(&self.author, &self.path))
            .key(FEED_ONLY_APP_POSTS_KEY)
            .arg(self.json()?)
            .arg(epoch_seed())
            .arg(self.uri())
            .arg(if feed_only { "1" } else { "0" })
            .invoke_async::<()>(&mut get_redis_conn().await?)
            .await?;
        Ok(())
    }

    pub async fn feed_only(uri: &str) -> RedisResult<Option<Self>> {
        let raw: Option<String> = get_redis_conn()
            .await?
            .hget(FEED_ONLY_APP_POSTS_KEY, uri)
            .await?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|e| RedisError::DeserializationFailed(Box::new(e)))
        })
        .transpose()
    }

    pub async fn prepare_delete(&self) -> RedisResult<()> {
        let _: bool = get_redis_conn()
            .await?
            .set_nx(pending_key(&self.author, &self.path), self.json()?)
            .await?;
        Ok(())
    }

    /// Pending metadata survives removal of the node. A retried DEL can finish
    /// even when neither the graph node nor feed_only record still exists.
    pub async fn finish_delete(post: &AppPostUri) -> RedisResult<()> {
        static FINISH: LazyLock<Script> = LazyLock::new(|| {
            Script::new(
                r"
            local line = redis.call('GET', KEYS[3])
            if line then
                redis.call('SETNX', KEYS[2], ARGV[1])
                redis.call('RPUSH', KEYS[1], line)
                redis.call('DEL', KEYS[3])
                redis.call('HDEL', KEYS[4], ARGV[2])
            end
        ",
            )
        });
        FINISH
            .key(RESOURCE_CHANGES_KEY)
            .key(EPOCH_KEY)
            .key(pending_key(post.author.as_ref(), &post.path()))
            .key(FEED_ONLY_APP_POSTS_KEY)
            .arg(epoch_seed())
            .arg(post.uri())
            .invoke_async::<()>(&mut get_redis_conn().await?)
            .await?;
        Ok(())
    }
}
