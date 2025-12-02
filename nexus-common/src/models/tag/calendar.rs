use crate::db::RedisOps;
use async_trait::async_trait;
use neo4rs::Query;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::traits::TagCollection;
use super::traits::TaggersCollection;
use crate::db::queries;

pub const CALENDAR_TAGS_KEY_PARTS: [&str; 2] = ["Calendars", "Tag"];

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, Default)]
pub struct TagCalendar(pub Vec<String>);

impl AsRef<[String]> for TagCalendar {
    fn as_ref(&self) -> &[String] {
        &self.0
    }
}

#[async_trait]
impl RedisOps for TagCalendar {
    async fn prefix() -> String {
        String::from("Calendar:Taggers")
    }
}

impl TagCollection for TagCalendar {
    fn get_tag_prefix<'a>() -> [&'a str; 2] {
        CALENDAR_TAGS_KEY_PARTS
    }

    fn read_graph_query(user_id: &str, extra_param: Option<&str>) -> Query {
        match extra_param {
            Some(calendar_id) => queries::get::calendar_tags(user_id, calendar_id),
            None => panic!("Calendar tags require calendar_id parameter"),
        }
    }
}

impl TaggersCollection for TagCalendar {}
