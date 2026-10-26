use crate::errors::EventProcessorError;
use nexus_common::config::watcher::AppKindsConfig;
use nexus_common::models::app_post::{is_post_reference, AppPost, AppPostInput, AppPostUri};
use nexus_common::models::event::resource_changes::{ResourceChange, ResourceChangeOp};
use std::collections::BTreeMap;

pub async fn put(
    path: &AppPostUri,
    bytes: &[u8],
    kinds: &BTreeMap<String, AppKindsConfig>,
) -> Result<(), EventProcessorError> {
    let input = path
        .read(bytes)
        .map_err(EventProcessorError::SpecValidation)?;
    sync_put(path, input, kinds).await
}

pub async fn sync_put(
    path: &AppPostUri,
    input: AppPostInput,
    kinds: &BTreeMap<String, AppKindsConfig>,
) -> Result<(), EventProcessorError> {
    let feed_only = kinds
        .get(&path.namespace)
        .is_some_and(|config| config.feed_only.contains(&input.kind.0));
    let post = AppPost::new(path, input);
    if feed_only {
        // A kind edit may move a formerly durable object into the feed_only tier.
        if AppPost::get(&post.uri).await?.is_some() {
            AppPost::delete(&post.uri).await?;
        }
    } else if !post.put().await? {
        let mut dependencies = vec![pubky_social_specs::legacy_v0::user_uri_builder(
            post.author.clone(),
        )];
        dependencies.extend(
            [post.parent.clone(), post.embed.clone()]
                .into_iter()
                .flatten()
                .filter(|uri| is_post_reference(uri)),
        );
        return Err(EventProcessorError::missing_dependencies(dependencies));
    }
    ResourceChange::post(path, ResourceChangeOp::Put)?
        .store()
        .await?;
    Ok(())
}

pub async fn del(path: &AppPostUri) -> Result<(), EventProcessorError> {
    let uri = path.uri();
    // All app versions name their own URI, even if this indexer never saw the PUT.
    // Persist the exact file DEL before changing graph metadata. Retried DELs are
    // harmless duplicates under the resource feed's author/object_path contract.
    ResourceChange::post(path, ResourceChangeOp::Del)?
        .prepare_delete()
        .await?;
    AppPost::delete(&uri).await?;
    ResourceChange::finish_delete(path.author.as_ref(), &path.version_uri()).await?;
    Ok(())
}
