use crate::events::errors::EventProcessorError;
use crate::events::retry::event::RetryEvent;
use crate::handle_indexing_results;
use nexus_common::db::OperationOutcome;
use nexus_common::models::attendee::AttendeeDetails;
use nexus_common::models::homeserver::Homeserver;
use nexus_common::types::DynError;
use pubky_app_specs::{PubkyAppAttendee, PubkyId};
use tracing::debug;

pub async fn sync_put(
    attendee: PubkyAppAttendee,
    author_id: PubkyId,
    attendee_id: String,
) -> Result<(), DynError> {
    debug!("Indexing new attendee: {}/{}", author_id, attendee_id);

    // Create AttendeeDetails object
    let attendee_details =
        AttendeeDetails::from_homeserver(attendee.clone(), &author_id, &attendee_id).await?;

    let existed = match attendee_details.put_to_graph().await? {
        OperationOutcome::CreatedOrDeleted => false,
        OperationOutcome::Updated => true,
        OperationOutcome::MissingDependency => {
            // Check if we need to ingest the event that this attendee is RSVPing to
            let mut dependency_event_keys = Vec::new();

            let event_uri = &attendee.x_pubky_event_uri;
            if let Some(key) = RetryEvent::generate_index_key(event_uri) {
                dependency_event_keys.push(key);
            }

            // Try to ingest the event's homeserver
            if let Err(e) = Homeserver::maybe_ingest_for_event(event_uri).await {
                tracing::error!("Failed to ingest homeserver for event: {e}");
            }

            if dependency_event_keys.is_empty() {
                return Err(EventProcessorError::SkipIndexing.into());
            }

            return Err(EventProcessorError::missing_dependencies(dependency_event_keys).into());
        }
    };

    if existed {
        // If the attendee existed, this is an update (RSVP status change)
        debug!("Attendee {}/{} updated", author_id, attendee_id);
    }

    // Save to Redis index
    if let Err(e) = attendee_details
        .put_to_index(&author_id, &attendee_id)
        .await
    {
        return Err(EventProcessorError::IndexWriteFailed {
            message: format!("attendee index write failed - {:?}", e.to_string()),
        }
        .into());
    }

    Ok(())
}

pub async fn del(author_id: PubkyId, attendee_id: String) -> Result<(), DynError> {
    debug!("Deleting attendee: {}/{}", author_id, attendee_id);

    let indexing_results = tokio::join!(AttendeeDetails::delete(&author_id, &attendee_id));

    handle_indexing_results!(indexing_results.0);

    Ok(())
}
