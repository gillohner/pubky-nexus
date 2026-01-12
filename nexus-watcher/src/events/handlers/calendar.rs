use nexus_common::models::event::EventProcessorError;
use crate::handle_indexing_results;
use nexus_common::db::OperationOutcome;
use nexus_common::models::calendar::CalendarDetails;
use nexus_common::types::DynError;
use pubky_app_specs::{PubkyAppCalendar, PubkyId};
use tracing::debug;

pub async fn sync_put(
    calendar: PubkyAppCalendar,
    author_id: PubkyId,
    calendar_id: String,
) -> Result<(), DynError> {
    debug!("Indexing new calendar: {}/{}", author_id, calendar_id);
    
    // Create CalendarDetails object
    let calendar_details =
        CalendarDetails::from_homeserver(calendar, &author_id, &calendar_id).await?;

    let existed = match calendar_details.put_to_graph().await? {
        OperationOutcome::CreatedOrDeleted => false,
        OperationOutcome::Updated => true,
        OperationOutcome::MissingDependency => {
            return Err(EventProcessorError::SkipIndexing.into());
        }
    };

    if existed {
        // If the calendar existed, this is an edit - reindex from graph to ensure consistency
        debug!("Calendar {}/{} updated, reindexing", author_id, calendar_id);
        if let Err(e) = CalendarDetails::reindex(&author_id, &calendar_id).await {
            return Err(EventProcessorError::IndexWriteFailed {
                message: format!("calendar reindex failed - {:?}", e.to_string()),
            }
            .into());
        }
    } else {
        // Save to Redis index for new calendars
        if let Err(e) = calendar_details
            .put_to_index(&author_id, &calendar_id)
            .await
        {
            return Err(EventProcessorError::IndexWriteFailed {
                message: format!("calendar index write failed - {:?}", e.to_string()),
            }
            .into());
        }
    }

    Ok(())
}

pub async fn del(author_id: PubkyId, calendar_id: String) -> Result<(), DynError> {
    debug!("Deleting calendar: {}/{}", author_id, calendar_id);

    let indexing_results = tokio::join!(CalendarDetails::delete(&author_id, &calendar_id));

    handle_indexing_results!(indexing_results.0);

    Ok(())
}
