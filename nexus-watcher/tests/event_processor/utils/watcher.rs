// Re-export the shared test harness from the library's `testing` feature.
pub use nexus_watcher::testing::{
    generate_post_id, HomeserverHashIdPath, HomeserverIdPath, HomeserverPath,
    HomeserverPathForPubkyId, WatcherTest,
};

// ── Nexus-watcher-specific test helpers ──────────────────────────────────────
// These rely on internal event-processor types and are not needed by downstream
// plugin tests.

use nexus_common::get_files_dir_pathbuf;
use nexus_common::models::event::{Event, EventProcessorError, ParseResult};
use nexus_common::models::file::FileDetails;
use nexus_common::models::traits::Collection;
use nexus_watcher::events::retry::event::RetryEvent;
use nexus_watcher::events::{handle, Moderation};
use pubky_app_specs::file_uri_builder;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

/// Retrieves an event from the homeserver and handles it asynchronously.
pub async fn retrieve_and_handle_event_line(
    event_line: &str,
    moderation: Arc<Moderation>,
) -> Result<(), EventProcessorError> {
    match Event::parse_event(event_line, get_files_dir_pathbuf())? {
        ParseResult::Parsed(event) => handle(&event, moderation).await,
        ParseResult::Skipped => Ok(()),

        // Propagate UnrecognizedUri as error, because this test helper is only meant for standard event handling
        ParseResult::UnrecognizedUri { reason, .. } => Err(EventProcessorError::InvalidEventLine(
            format!("Cannot parse event URI: {reason}"),
        )),
    }
}

/// Polls the retry index until the entry appears or the timeout is reached.
pub async fn assert_eventually_exists(event_index: &str) {
    const SLEEP_MS: u64 = 3;
    const MAX_RETRIES: usize = 50;

    for attempt in 0..MAX_RETRIES {
        debug!(
            "RetryEvent: Trying to read index {:?}, attempt {}/{} ({}ms)",
            event_index,
            attempt + 1,
            MAX_RETRIES,
            SLEEP_MS * attempt as u64
        );
        match RetryEvent::check_uri(event_index).await {
            Ok(timeframe) => {
                if timeframe.is_some() {
                    return;
                }
            }
            Err(e) => panic!("Error while getting index: {e:?}"),
        };
        tokio::time::sleep(Duration::from_millis(SLEEP_MS)).await;
    }
    panic!("TIMEOUT: It takes too long to read the RetryManager new index")
}

/// Common assertions for `FileDetails` of an existing file.
pub async fn assert_file_details(
    user_id: &str,
    file_id: &str,
    blob_absolute_url: &str,
    file: &pubky_app_specs::PubkyAppFile,
) -> FileDetails {
    let file_absolute_url = file_uri_builder(user_id.into(), file_id.into());

    let files = FileDetails::get_by_ids(vec![vec![user_id, file_id].as_slice()].as_slice())
        .await
        .expect("Failed to fetch files from Nexus");

    let result_file = files[0].as_ref().expect("Created file was not found.");

    assert_eq!(result_file.id, file_id);
    assert_eq!(result_file.src, blob_absolute_url);
    assert_eq!(result_file.uri, file_absolute_url);
    assert_eq!(result_file.size, file.size as i64);
    assert_eq!(result_file.name, file.name);
    assert_eq!(result_file.owner_id, user_id);

    result_file.clone()
}
