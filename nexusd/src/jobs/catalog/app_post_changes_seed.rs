use crate::jobs::Job;
use async_trait::async_trait;
use nexus_common::models::event::app_post_changes::AppPostChange;
use nexus_common::types::DynError;

/// Regenerates the app-post feed from current versions and graph metadata, retaining live appends.
pub struct AppPostChangesSeedJob;

#[async_trait]
impl Job for AppPostChangesSeedJob {
    fn name(&self) -> &'static str {
        "app-post-changes-seed"
    }

    async fn run(&self) -> Result<(), DynError> {
        let lines = AppPostChange::reseed().await?;
        tracing::info!(lines, "App post changes feed seeded; epoch advanced");
        Ok(())
    }
}
