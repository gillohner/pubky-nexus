//! Recovery of users whose `social/v1` profile this Nexus never indexed.
//!
//! The epoch gate reads a user through `social/v1` only once their `social/v1` profile PUT has
//! flipped them to `v1` (see `events::gate`). The app's migrator, and a new account's signup,
//! write that profile once. When the PUT reached the index before this Nexus could flip on
//! it (an index upgraded in place from a build without the flip, or a homeserver another
//! build was watching at the time), the user stays at `v0`, or is not in the graph at all,
//! and every later `social/v1` write of theirs is ignored: tags on external links, posts,
//! replies, follows. The gate skips those events without an error or a retry, so nothing
//! shows the loss but the missing data.
//!
//! [`recover_v1_user`] replays such a user's own `social/v1` events from their homeserver's
//! per-user stream, in order, through the watcher's event handler. Events before the profile
//! PUT are ignored as they were live (the migrator's copies), the profile PUT flips the user,
//! and every later write is indexed: the index the user would have if this Nexus had watched
//! them from the start. An event handled before is handled again, as a retry would; a write
//! the stream later deletes is put and then deleted. Users already at `v1`, and users with no
//! `social/v1` profile on their homeserver (not migrated), are left alone.

use crate::errors::EventProcessorError;
use crate::events::{Event, EventHandler};
use crate::homeserver::{HomeserverClient, HsErrorKind};
use nexus_common::models::user::Epoch;
use pubky::PubkyResource;
use pubky_social_specs::PubkyId;
use tracing::{info, warn};

/// Events read per page of a user's stream.
const PAGE_SIZE: u16 = 1000;

/// What [`recover_v1_user`] did for one user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// The user is at `v1` already: nothing was replayed.
    AlreadyV1,
    /// The homeserver holds no `social/v1` profile for the user, who has not migrated:
    /// nothing was replayed.
    NoV1Profile,
    /// The user's `social/v1` events were replayed: `handled` went to the event handler, of
    /// which `failed` returned an error (logged, not retried), and the user is now at `epoch`.
    Replayed {
        handled: usize,
        failed: usize,
        epoch: Epoch,
    },
}

/// Replays `user`'s `social/v1` events from `homeserver` through `handler` when the user has a
/// `social/v1` profile there but is not at `v1` in the graph. See the module documentation.
pub async fn recover_v1_user(
    handler: &dyn EventHandler,
    client: &dyn HomeserverClient,
    homeserver: &PubkyId,
    user: &PubkyId,
) -> Result<Recovery, EventProcessorError> {
    if Epoch::get_from_graph(user).await? == Epoch::V1 {
        return Ok(Recovery::AlreadyV1);
    }

    let prefix = format!("pubky://{user}/pub/social/v1/");
    let profile: PubkyResource = format!("{prefix}profile.json").parse()?;
    match client.get_resource(&profile).await {
        Ok(_) => {}
        Err(error) if matches!(error.kind, HsErrorKind::NotFound(_)) => {
            return Ok(Recovery::NoV1Profile)
        }
        Err(error) => return Err(error.into()),
    }

    let user_pk = user
        .to_public_key()
        .map_err(|e| EventProcessorError::SpecValidation(format!("{user}: {e}")))?;
    let (mut cursor, mut handled, mut failed) = (0, 0, 0);
    loop {
        let page = client
            .user_events(homeserver, &user_pk, cursor, PAGE_SIZE)
            .await?;
        let page_len = page.len();
        for stream_event in &page {
            cursor = stream_event.cursor;
            // A user's stream holds only their own writes; anything else is not replayed.
            if stream_event.owner() != &user_pk || !stream_event.uri().starts_with(&prefix) {
                continue;
            }
            let event = match Event::from_stream_event(stream_event) {
                Ok(Some(event)) => event,
                Ok(None) => continue,
                Err(error) => {
                    warn!(%user, uri = stream_event.uri(), %error, "Skipping unparseable stream event");
                    continue;
                }
            };
            handled += 1;
            if let Err(error) = handler.handle(&event).await {
                failed += 1;
                warn!(%user, uri = %event.uri, %error, "Replayed event failed");
            }
        }
        if page_len < usize::from(PAGE_SIZE) {
            break;
        }
    }

    let epoch = Epoch::get_from_graph(user).await?;
    info!(%user, handled, failed, ?epoch, "Replayed the user's social/v1 events");
    Ok(Recovery::Replayed {
        handled,
        failed,
        epoch,
    })
}

/// [`recover_v1_user`] for each of `users` in turn, with each one's outcome.
pub async fn recover_v1_users(
    handler: &dyn EventHandler,
    client: &dyn HomeserverClient,
    homeserver: &PubkyId,
    users: &[PubkyId],
) -> Vec<(PubkyId, Result<Recovery, EventProcessorError>)> {
    let mut outcomes = Vec::with_capacity(users.len());
    for user in users {
        let outcome = recover_v1_user(handler, client, homeserver, user).await;
        outcomes.push((user.clone(), outcome));
    }
    outcomes
}
