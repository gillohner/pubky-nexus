use crate::db::RedisOps;
use async_trait::async_trait;
use neo4rs::Query;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::traits::TagCollection;
use super::traits::TaggersCollection;
use crate::db::queries;

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

    fn read_graph_query(user_id: &str, extra_param: Option<&str>) -> Query {
        match extra_param {
            Some(event_id) => queries::get::event_tags(user_id, event_id),
            None => panic!("Event tags require event_id parameter"),
        }
    }
}

impl TaggersCollection for TagEvent {}
