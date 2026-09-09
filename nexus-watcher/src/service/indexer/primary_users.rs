//! Primary users have one event owner: the global stream before tracking, then the per-user stream.
//! Serialize the handoff check and handler effects so old global deletes cannot overtake newer puts.
use tokio::sync::{Mutex, MutexGuard};

static PROCESSING: Mutex<()> = Mutex::const_new(());

pub(crate) async fn processing_guard(enabled: bool) -> Option<MutexGuard<'static, ()>> {
    if enabled {
        Some(PROCESSING.lock().await)
    } else {
        None
    }
}

pub(super) fn eligible_cursor(cursor: Option<u64>, primary: bool) -> Option<u64> {
    cursor.or_else(|| (!primary).then_some(0))
}

pub(crate) async fn is_tracked(
    user_id: &str,
    hs_id: &str,
) -> Result<bool, crate::errors::EventProcessorError> {
    let cursors = nexus_common::models::user::UserHsCursor::read_tracked(&[user_id], hs_id).await?;
    Ok(cursors.first().is_some_and(Option::is_some))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    fn primary_lane_requires_explicit_tracking_without_skipping_zero() {
        assert_eq!(eligible_cursor(None, true), None);
        assert_eq!(eligible_cursor(Some(0), true), Some(0));
        assert_eq!(eligible_cursor(Some(42), true), Some(42));
        assert_eq!(eligible_cursor(None, false), Some(0));
    }

    #[tokio::test]
    async fn handoff_waits_for_the_in_flight_global_effect_before_the_newer_effect() {
        let global = processing_guard(true).await;
        let applied = Arc::new(AtomicBool::new(false));
        let observed = applied.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let next = tokio::spawn(async move {
            started_tx.send(()).unwrap();
            let _guard = processing_guard(true).await;
            assert!(
                observed.load(Ordering::SeqCst),
                "newer per-user effect overtook global work"
            );
        });
        started_rx.await.unwrap();
        assert!(!next.is_finished());
        applied.store(true, Ordering::SeqCst);
        drop(global);
        next.await.unwrap();
    }

    #[tokio::test]
    async fn disabled_processors_do_not_wait_for_primary_routing() {
        let _enabled = processing_guard(true).await;
        assert!(processing_guard(false).await.is_none());
    }
    struct PendingPrimary {
        release: Arc<tokio::sync::Notify>,
        completion: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    }

    #[async_trait::async_trait]
    impl super::super::TEventProcessor for PendingPrimary {
        fn event_handler(&self) -> &Arc<dyn crate::events::EventHandler> {
            unreachable!("this timeout test does not invoke event handlers")
        }
        fn instance_name(&self) -> &'static str {
            "PendingPrimary"
        }
        fn primary_user_indexing(&self) -> bool {
            true
        }
        fn custom_timeout(&self) -> Option<std::time::Duration> {
            Some(std::time::Duration::from_millis(10))
        }
        async fn run_internal(self: Arc<Self>) -> Result<(), crate::errors::EventProcessorError> {
            self.release.notified().await;
            let _ = self.completion.lock().unwrap().take().unwrap().send(());
            Ok(())
        }
    }

    #[tokio::test]
    async fn primary_timeout_joins_cancellation_before_another_poll_can_start() {
        use super::super::TEventProcessor;
        let release = Arc::new(tokio::sync::Notify::new());
        let (completion, completed) = tokio::sync::oneshot::channel();
        let processor = Arc::new(PendingPrimary {
            release: release.clone(),
            completion: std::sync::Mutex::new(Some(completion)),
        });
        assert!(processor.run().await.unwrap_err().is_timeout());
        release.notify_one();
        assert!(
            completed.await.is_err(),
            "timed-out task survived and applied a late effect"
        );
    }
}
