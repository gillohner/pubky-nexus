use super::utils::{find_user_epoch, set_user_epoch};
use crate::event_processor::posts::utils::{test_user, v1_test_user};
use crate::event_processor::tags::resource_utils::{compute_resource_id, find_resource_tag};
use crate::event_processor::utils::watcher::WatcherTest;
use anyhow::Result;
use nexus_watcher::service::recover_v1::{recover_v1_user, Recovery};
use pubky::Keypair;
use pubky_social_specs::{PubkyId, PubkySocialTag};

/// The external link the tests tag, as the app writes it.
const LINK: &str = "https://www.openstreetmap.org/node/270803095";

/// Recovers `user_id` on the test homeserver with the watcher's own handler and client.
async fn recover(test: &WatcherTest, user_id: &str) -> Result<Recovery> {
    let runner = &test.event_processor_runner;
    let user = PubkyId::try_from(user_id).map_err(anyhow::Error::msg)?;
    Ok(recover_v1_user(
        runner.event_handler.as_ref(),
        runner.client.as_ref(),
        &test.homeserver_id,
        &user,
    )
    .await?)
}

/// A migrated user the index kept at `v0` (their profile PUT was never flipped on) has their
/// `social/v1` tag on an external link ignored; recovery flips them and indexes the tag, and a
/// second run changes nothing.
#[tokio_shared_rt::test(shared)]
async fn test_recover_v1_user_indexes_a_stranded_users_resource_tag() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;

    let user_kp = Keypair::random();
    let user_id = test
        .create_user(
            &user_kp,
            &test_user("Watcher:RecoverV1:Stranded", "recover_v1"),
        )
        .await?;
    test.put_v1_profile(
        &user_kp,
        &v1_test_user("Watcher:RecoverV1:V1", "recover_v1"),
    )
    .await?;
    // Standing in for an index that saw the profile PUT before it could flip on it
    set_user_epoch(&user_id, "v0").await?;

    let label = "recoverv1";
    let tag = PubkySocialTag::new(LINK.to_string(), label.to_string());
    let tag_path = test.put_v1_tag(&user_kp, &tag).await?;
    let resource_id = compute_resource_id(LINK);
    assert!(
        find_resource_tag(&resource_id, label).await?.is_none(),
        "a v0 user's social/v1 tag is ignored by the gate"
    );

    let outcome = recover(&test, &user_id).await?;
    assert!(
        matches!(outcome, Recovery::Replayed { failed: 0, epoch, .. } if epoch == nexus_common::models::user::Epoch::V1),
        "{outcome:?}"
    );
    assert_eq!(find_user_epoch(&user_id).await.as_deref(), Some("v1"));
    let indexed = find_resource_tag(&resource_id, label)
        .await?
        .expect("the replay indexes the tag");
    assert_eq!(indexed.tagger, user_id);

    assert_eq!(recover(&test, &user_id).await?, Recovery::AlreadyV1);

    // Handled at `v1`: the untag removes it
    test.del(&user_kp, &tag_path).await?;
    assert!(find_resource_tag(&resource_id, label).await?.is_none());
    set_user_epoch(&user_id, "v0").await?;
    test.cleanup_user(&user_kp).await?;
    Ok(())
}

/// A user the index never saw, whose first write was their `social/v1` profile, is created at
/// `v1` by the replay and gets the tag they wrote after it.
#[tokio_shared_rt::test(shared)]
async fn test_recover_v1_user_creates_an_unseen_v1_user() -> Result<()> {
    let test = WatcherTest::setup(None).await?;

    let user_kp = Keypair::random();
    let user_id = user_kp.public_key().to_z32();
    test.register_user(&user_kp).await?;
    // Written while no watcher ran
    let mut test = test.remove_event_processing().await;
    test.put_v1_profile(
        &user_kp,
        &v1_test_user("Watcher:RecoverV1:Unseen", "recover_v1"),
    )
    .await?;
    let label = "recoverunseen";
    let tag = PubkySocialTag::new(LINK.to_string(), label.to_string());
    test.put_v1_tag(&user_kp, &tag).await?;
    assert_eq!(find_user_epoch(&user_id).await, None, "no node yet");

    let outcome = recover(&test, &user_id).await?;
    assert!(
        matches!(outcome, Recovery::Replayed { failed: 0, .. }),
        "{outcome:?}"
    );
    assert_eq!(find_user_epoch(&user_id).await.as_deref(), Some("v1"));
    assert!(find_resource_tag(&compute_resource_id(LINK), label)
        .await?
        .is_some());
    Ok(())
}

/// A user without a `social/v1` profile has not migrated: recovery replays nothing and their
/// `social/v1` tag stays ignored.
#[tokio_shared_rt::test(shared)]
async fn test_recover_v1_user_leaves_an_unmigrated_user_alone() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;

    let user_kp = Keypair::random();
    let user_id = test
        .create_user(&user_kp, &test_user("Watcher:RecoverV1:V0", "recover_v1"))
        .await?;
    let label = "recovernotmigrated";
    let tag = PubkySocialTag::new(LINK.to_string(), label.to_string());
    let tag_path = test.put_v1_tag(&user_kp, &tag).await?;

    assert_eq!(recover(&test, &user_id).await?, Recovery::NoV1Profile);
    // A user born `v0` may carry no stored epoch: either way not `v1`
    assert_ne!(find_user_epoch(&user_id).await.as_deref(), Some("v1"));
    assert!(find_resource_tag(&compute_resource_id(LINK), label)
        .await?
        .is_none());

    test.del(&user_kp, &tag_path).await?;
    test.cleanup_user(&user_kp).await?;
    Ok(())
}
