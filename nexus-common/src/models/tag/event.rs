use crate::db::RedisOps;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::traits::TagCollection;
use super::traits::TaggersCollection;

pub const EVENT_TAGS_KEY_PARTS: [&str; 2] = ["Events", "Tag"];

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, Default)]
pub struct TagEvent(pub Vec<String>);

impl AsRef<[String]> for TagEvent {
    fn as_ref(&self) -> &[String] {
        &self.0
    }
}

#[async_trait]
impl RedisOps for TagEvent {
    async fn prefix() -> String {
        String::from("Event:Taggers")
    }
}

impl TagCollection for TagEvent {
    fn get_tag_prefix<'a>() -> [&'a str; 2] {
        EVENT_TAGS_KEY_PARTS
    }
}

impl TaggersCollection for TagEvent {}
