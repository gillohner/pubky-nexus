use super::{HomeserverBackoff, TEventProcessorRunner};
use crate::events::retry::RetryScheduler;
use crate::events::{DefaultEventHandler, EventHandler};
use crate::service::indexer::{HsEventProcessor, TEventProcessor};
use crate::service::stats::{ProcessedStats, ProcessorRunStatus, RunAllProcessorsStats};
use nexus_common::models::homeserver::Homeserver;
use nexus_common::types::DynError;
use nexus_common::WatcherConfig;
use pubky_app_specs::PubkyId;
use std::sync::Arc;
use tokio::sync::{watch::Receiver, Mutex};
use tracing::debug;

pub struct HsEventProcessorRunner {
    /// See [WatcherConfig::events_limit]
    pub limit: u16,

    /// Failed polls retain their cursor and pause before the next attempt.
    pub backoff: Mutex<HomeserverBackoff>,

    /// Opt-in exclusive routing of explicitly tracked primary users.
    pub primary_user_indexing: bool,

    pub event_handler: Arc<dyn EventHandler>,
    pub shutdown_rx: Receiver<bool>,

    /// See [WatcherConfig::homeserver]
    pub primary_homeserver: PubkyId,

    /// Scheduler shared with every processor this runner builds
    pub retry_scheduler: Arc<RetryScheduler>,
}

impl HsEventProcessorRunner {
    /// Creates a new instance from the provided configuration
    pub fn from_config(config: &WatcherConfig, shutdown_rx: Receiver<bool>) -> Self {
        Self {
            limit: config.events_limit,
            backoff: Mutex::new(HomeserverBackoff::new(
                config.initial_backoff_secs,
                config.max_backoff_secs,
            )),
            primary_user_indexing: config.primary_user_indexing,
            event_handler: Arc::new(DefaultEventHandler::from_config(config)),
            shutdown_rx,
            primary_homeserver: config.homeserver.clone(),
            retry_scheduler: Arc::new(RetryScheduler::from_config(config)),
        }
    }

    pub fn primary_homeserver(&self) -> &str {
        &self.primary_homeserver
    }
}

#[async_trait::async_trait]
impl TEventProcessorRunner for HsEventProcessorRunner {
    fn shutdown_rx(&self) -> Receiver<bool> {
        self.shutdown_rx.clone()
    }

    /// Creates and returns a new event processor instance for the specified homeserver
    async fn build(&self, homeserver_id: &str) -> Result<Arc<dyn TEventProcessor>, DynError> {
        let homeserver_id = PubkyId::try_from(homeserver_id)?;
        let homeserver = Homeserver::get_by_id(homeserver_id)
            .await?
            .ok_or("Homeserver not found")?;

        Ok(Arc::new(HsEventProcessor {
            homeserver,
            limit: self.limit,
            primary_user_indexing: self.primary_user_indexing,
            event_handler: self.event_handler.clone(),
            shutdown_rx: self.shutdown_rx.clone(),
            retry_scheduler: self.retry_scheduler.clone(),
            hs_mapping_cache: Default::default(),
        }))
    }

    async fn pre_run(&self) -> Result<Vec<String>, DynError> {
        Ok(vec![self.primary_homeserver.to_string()])
    }

    async fn backoff_hs_should_skip(&self, hs_id: &str) -> bool {
        self.backoff.lock().await.should_skip(hs_id)
    }

    async fn backoff_hs_record_result(&self, hs_id: &str, status: &ProcessorRunStatus) {
        let mut backoff = self.backoff.lock().await;
        if *status == ProcessorRunStatus::Ok {
            backoff.record_success(hs_id);
        } else {
            backoff.record_failure(hs_id);
        }
    }

    async fn post_run(&self, stats: RunAllProcessorsStats) -> ProcessedStats {
        for individual_run_stat in &stats.stats {
            let hs_id = &individual_run_stat.hs_id;
            let duration = individual_run_stat.duration;
            let status = &individual_run_stat.status;
            debug!(homeserver = %hs_id, ?duration, ?status, "Primary homeserver run completed");
        }

        ProcessedStats(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_primary_poll_is_paused_and_success_resets_backoff() {
        let (_, shutdown) = tokio::sync::watch::channel(false);
        let runner = HsEventProcessorRunner::from_config(&WatcherConfig::default(), shutdown);
        let hs = runner.primary_homeserver();
        assert!(!runner.backoff_hs_should_skip(hs).await);
        runner
            .backoff_hs_record_result(hs, &ProcessorRunStatus::Error)
            .await;
        assert!(runner.backoff_hs_should_skip(hs).await);
        runner
            .backoff_hs_record_result(hs, &ProcessorRunStatus::Ok)
            .await;
        assert!(!runner.backoff_hs_should_skip(hs).await);
    }
}
