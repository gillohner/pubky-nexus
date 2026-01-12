use nexus_common::models::event::EventProcessorError;
use crate::events::retry::event::RetryEvent;
use crate::handle_indexing_results;
use nexus_common::db::OperationOutcome;
use nexus_common::models::event::EventDetails;
use nexus_common::models::homeserver::Homeserver;
use nexus_common::types::DynError;
use pubky_app_specs::{PubkyAppEvent, PubkyId};
use tracing::debug;

pub async fn sync_put(
    event: PubkyAppEvent,
    author_id: PubkyId,
    event_id: String,
) -> Result<(), DynError> {
    debug!("Indexing new event: {}/{}", author_id, event_id);

    // Create EventDetails object
    let event_details = EventDetails::from_homeserver(event.clone(), &author_id, &event_id).await?;

    let existed = match event_details.put_to_graph().await? {
        OperationOutcome::CreatedOrDeleted => false,
        OperationOutcome::Updated => true,
        OperationOutcome::MissingDependency => {
            // Check if we need to ingest calendars that this event belongs to
            let mut dependency_event_keys = Vec::new();

            if let Some(calendar_uris) = &event.x_pubky_calendar_uris {
                for calendar_uri in calendar_uris {
                    if let Some(key) = RetryEvent::generate_index_key(calendar_uri) {
                        dependency_event_keys.push(key);
                    }

                    // Try to ingest the calendar's homeserver
                    if let Err(e) = Homeserver::maybe_ingest_for_calendar(calendar_uri).await {
                        tracing::error!("Failed to ingest homeserver for calendar: {e}");
                    }
                }
            }

            if dependency_event_keys.is_empty() {
                return Err(EventProcessorError::SkipIndexing.into());
            }

            return Err(EventProcessorError::missing_dependencies(dependency_event_keys).into());
        }
    };

    if existed {
        // If the event existed, this is an edit - reindex from graph to ensure consistency
        debug!("Event {}/{} updated, reindexing", author_id, event_id);
        if let Err(e) = EventDetails::reindex(&author_id, &event_id).await {
            return Err(EventProcessorError::IndexWriteFailed {
                message: format!("event reindex failed - {:?}", e.to_string()),
            }
            .into());
        }
    } else {
        // Save to Redis index for new events
        if let Err(e) = event_details.put_to_index(&author_id, &event_id).await {
            return Err(EventProcessorError::IndexWriteFailed {
                message: format!("event index write failed - {:?}", e.to_string()),
            }
            .into());
        }
    }

    Ok(())
}

pub async fn del(author_id: PubkyId, event_id: String) -> Result<(), DynError> {
    debug!("Deleting event: {}/{}", author_id, event_id);

    let indexing_results = tokio::join!(EventDetails::delete(&author_id, &event_id));

    handle_indexing_results!(indexing_results.0);

    Ok(())
}
