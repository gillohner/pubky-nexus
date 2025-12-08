use crate::db::RedisOps;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::traits::TagCollection;
use super::traits::TaggersCollection;

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
}

impl TaggersCollection for TagCalendar {}
