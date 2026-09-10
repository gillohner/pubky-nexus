//! Reconcile current source mentions using native notification and graph storage.
use std::collections::HashSet;

use nexus_common::db::{exec_single_row, queries};
use nexus_common::models::notification::Notification;
use nexus_common::models::post::{social::custom_social_text, PostKind, PostRelationships};
use pubky_app_specs::PubkyId;

use crate::events::EventProcessorError;

const MAX_CUSTOM_MENTIONS: usize = 32;

pub(super) struct MentionPost<'a> {
    pub author_id: &'a PubkyId,
    pub post_id: &'a str,
    pub kind: &'a PostKind,
}

/// The source PUT clears prior mention edges before this replayable rebuild.
pub(super) async fn synchronize_mentions(
    post: MentionPost<'_>,
    content: &str,
    relationships: &mut PostRelationships,
) -> Result<(), EventProcessorError> {
    relationships.mentioned.clear();
    for mentioned in mentioned_ids(post.kind, content) {
        // Native posts historically retain their self-reference edge, but never
        // notify the author. Custom posts exclude that edge as before.
        if &mentioned == post.author_id && post.kind.known().is_none() {
            continue;
        }
        publish_mention(&post, &mentioned).await?;
        if &mentioned != post.author_id {
            relationships.mentioned.push(mentioned);
        }
    }
    Ok(())
}

async fn publish_mention(
    post: &MentionPost<'_>,
    mentioned: &PubkyId,
) -> Result<(), EventProcessorError> {
    exec_single_row(queries::put::create_mention_relationship(
        post.author_id,
        post.post_id,
        mentioned,
    ))
    .await?;
    Notification::new_mention_once(post.author_id, mentioned, post.post_id, post.kind.clone())
        .await?;
    Ok(())
}

/// Built-in text retains its native extraction and has no custom recipient cap.
pub(super) fn mentioned_ids(kind: &PostKind, content: &str) -> Vec<PubkyId> {
    if kind.known().is_some() {
        return text_mentions(content).collect();
    }
    custom_social_text(content)
        .map(|text| text_mentions(&text).take(MAX_CUSTOM_MENTIONS).collect())
        .unwrap_or_default()
}

fn text_mentions(text: &str) -> impl Iterator<Item = PubkyId> + '_ {
    let mut seen = HashSet::new();
    find_mentioned_ids(text, "pk:")
        .into_iter()
        .chain(find_mentioned_ids(text, "pubky"))
        .filter(move |id| seen.insert(id.to_string()))
}

fn find_mentioned_ids(content: &str, prefix: &str) -> Vec<PubkyId> {
    const PUBKY_ID_LENGTH: usize = 52;
    content
        .match_indices(prefix)
        .filter_map(|(start, _)| {
            let offset = start + prefix.len();
            content
                .get(offset..offset + PUBKY_ID_LENGTH)
                .and_then(|id| PubkyId::try_from(id).ok())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user() -> PubkyId {
        pubky::Keypair::random().into()
    }
    fn custom_kind() -> PostKind {
        "event".parse().unwrap()
    }
    fn social(text: impl AsRef<str>) -> String {
        json!({"social":{"version":1,"text":text.as_ref()}}).to_string()
    }

    #[test]
    fn metadata_never_supplies_custom_mentions() {
        let id = user();
        let content = json!({"organizer":format!("pubky{id}"),"description":format!("pk:{id}"),"calendar_uris":[format!("pubky://{id}/pub/pubky.app/posts/example")]});
        assert!(mentioned_ids(&custom_kind(), &content.to_string()).is_empty());
    }

    #[test]
    fn social_text_deduplicates_native_and_legacy_mentions() {
        let id = user();
        assert_eq!(
            mentioned_ids(
                &custom_kind(),
                &social(format!("pubky{id} pk:{id} pubky{id}"))
            ),
            vec![id]
        );
    }

    #[test]
    fn switching_to_builtin_uses_current_plain_text() {
        let old = user();
        let current = user();
        assert_eq!(
            mentioned_ids(&custom_kind(), &social(format!("pubky{old}"))),
            vec![old]
        );
        assert_eq!(
            mentioned_ids(&PostKind::Short, &format!("pubky{current}")),
            vec![current]
        );
        assert!(mentioned_ids(&PostKind::Short, "[DELETED]").is_empty());
    }

    #[test]
    fn builtin_edits_replace_the_recipient_set() {
        let first = user();
        let second = user();
        assert_eq!(
            mentioned_ids(&PostKind::Long, &format!("pk:{first}")),
            vec![first]
        );
        assert_eq!(
            mentioned_ids(&PostKind::Long, &format!("pk:{second}")),
            vec![second]
        );
        assert!(mentioned_ids(&PostKind::Long, "No mentions remain").is_empty());
    }

    #[test]
    fn custom_cap_does_not_change_builtin_limits() {
        let ids: Vec<PubkyId> = (0..40).map(|_| user()).collect();
        let text = ids
            .iter()
            .map(|id| format!("pubky{id}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            mentioned_ids(&custom_kind(), &social(&text)),
            ids[..MAX_CUSTOM_MENTIONS]
        );
        assert_eq!(mentioned_ids(&PostKind::Long, &text), ids);
    }

    #[test]
    fn malformed_future_and_removed_social_extensions_clear_custom_recipients() {
        let id = user();
        for content in [
            "{".to_owned(),
            json!({"social":{"version":2,"text":format!("pubky{id}")}}).to_string(),
            json!({"description":format!("pubky{id}")}).to_string(),
        ] {
            assert!(mentioned_ids(&custom_kind(), &content).is_empty());
        }
    }

    #[test]
    fn unicode_boundaries_and_truncated_ids_are_safe() {
        let id = user();
        let content = format!("🌻 pubky{}é pk:🌻 pubky{id}", &id.to_string()[..51]);
        assert_eq!(mentioned_ids(&PostKind::Short, &content), vec![id]);
    }
}

#[cfg(test)]
mod live_tests;
