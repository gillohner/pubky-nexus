//! Test utilities for integration tests — available when the `testing` feature is enabled.
//!
//! External crates (e.g. domain plugins) can add
//! `nexus-watcher = { ..., features = ["testing"] }` as a dev-dependency to get
//! access to `WatcherTest` and the shared testnet infrastructure.

use anyhow::{anyhow, Error, Result};
use base32::{encode, Alphabet};
use chrono::Utc;
use nexus_common::db::PubkyConnector;
use nexus_common::get_files_dir_test_pathbuf;
use nexus_common::models::homeserver::Homeserver;
use nexus_common::plugin::{NexusPlugin, PluginContext};
use pubky::Keypair;
use pubky::PublicKey;
use pubky::ResourcePath;
use pubky_app_specs::{
    traits::{HasIdPath, HasPath, HashId},
    PubkyAppFile, PubkyAppFollow, PubkyAppPost, PubkyAppUser, PubkyId,
};
use pubky_testnet::Testnet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};

use crate::dispatcher::EventDispatcher;
use crate::events::Moderation;
use crate::service::{EventProcessorRunner, TEventProcessorRunner};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Shared testnet instance — created once per process, reused by all tests.
///
/// Each `Testnet::new()` creates an isolated DHT network. Because `PubkyConnector`
/// is a process-wide `OnceCell`, only the first SDK ever gets registered; subsequent
/// tests that create their own testnet would get an unreachable homeserver. Sharing
/// one `Testnet` lets every test create homeservers in the same DHT while using the
/// same SDK already stored in `PubkyConnector`.
static SHARED_TESTNET: OnceCell<Arc<Mutex<Testnet>>> = OnceCell::const_new();

/// Shared homeserver ID — one homeserver is created per process and reused by all
/// tests. Creating a new homeserver for every test exhausts the PostgreSQL connection
/// pool when tests interleave on the shared tokio runtime.
static SHARED_HOMESERVER_ID: OnceCell<String> = OnceCell::const_new();

/// Returns the process-wide `Testnet`, initialising it on first call.
/// Also seeds `PubkyConnector` with the shared SDK (idempotent after first call).
async fn shared_testnet() -> Arc<Mutex<Testnet>> {
    SHARED_TESTNET
        .get_or_init(|| async {
            let mut testnet = Testnet::new()
                .await
                .expect("failed to create shared testnet");
            testnet
                .create_http_relay()
                .await
                .expect("failed to create http relay");
            let sdk = testnet.sdk().expect("testnet SDK unavailable");
            PubkyConnector::init_from(sdk)
                .await
                .expect("failed to init PubkyConnector");
            Arc::new(Mutex::new(testnet))
        })
        .await
        .clone()
}

/// Returns the shared homeserver's z32-encoded public key, creating it on first call.
async fn shared_homeserver_id() -> String {
    let testnet = shared_testnet().await;
    SHARED_HOMESERVER_ID
        .get_or_init(|| async {
            let mut t = testnet.lock().await;
            t.create_random_homeserver()
                .await
                .expect("failed to create shared homeserver")
                .public_key()
                .z32()
        })
        .await
        .clone()
}

/// Generate a unique post ID for tests.
/// Uses PID-based offset for inter-process uniqueness and atomic counter
/// for intra-process uniqueness.
pub fn generate_post_id() -> String {
    let now = Utc::now().timestamp_micros() as u64;
    let pid_offset = (std::process::id() as u64) * 1000;
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = now + pid_offset + count;
    let bytes = timestamp.to_be_bytes();
    encode(Alphabet::Crockford, &bytes)
}

fn default_moderation() -> Moderation {
    let id = PubkyId::try_from("uo7jgkykft4885n8cruizwy6khw71mnu5pq3ay9i8pw1ymcn85ko")
        .expect("hardcoded test moderation key should be valid");
    Moderation {
        id,
        tags: vec!["label_to_moderate".to_string()],
    }
}

/// Test harness that wraps a shared testnet + homeserver and an `EventProcessorRunner`.
///
/// Construct with [`WatcherTest::setup`] for plain nexus tests or
/// [`WatcherTest::setup_with_plugins`] when domain plugins must be active.
pub struct WatcherTest {
    pub testnet: Arc<Mutex<Testnet>>,
    /// The homeserver ID
    pub homeserver_id: String,
    /// The event processor runner
    pub event_processor_runner: EventProcessorRunner,
    /// Whether to ensure event processing is complete after each write
    pub ensure_event_processing: bool,
}

impl WatcherTest {
    fn create_test_event_processor_runner(default_homeserver: PubkyId) -> EventProcessorRunner {
        let moderation = Arc::new(default_moderation());
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        EventProcessorRunner {
            limit: 1000,
            monitored_homeservers_limit: 100,
            files_path: get_files_dir_test_pathbuf(),
            tracer_name: String::from("watcher.test"),
            moderation,
            shutdown_rx,
            default_homeserver,
            dispatcher: None,
        }
    }

    async fn init_stack_and_homeserver() -> Result<(Arc<Mutex<Testnet>>, String, PubkyId)> {
        if let Err(e) =
            nexus_common::StackManager::setup(&nexus_common::StackConfig::default()).await
        {
            return Err(Error::msg(format!("could not initialise the stack, {e:?}")));
        }
        let testnet = shared_testnet().await;
        let homeserver_id = shared_homeserver_id().await;
        let pubky_id = PubkyId::try_from(&homeserver_id).unwrap();
        Homeserver::persist_if_unknown(pubky_id.clone())
            .await
            .unwrap();
        Ok((testnet, homeserver_id, pubky_id))
    }

    /// Sets up the test environment without any domain plugins.
    pub async fn setup() -> Result<Self> {
        let (testnet, homeserver_id, pubky_id) = Self::init_stack_and_homeserver().await?;
        let event_processor_runner = Self::create_test_event_processor_runner(pubky_id);
        Ok(Self {
            testnet,
            homeserver_id,
            event_processor_runner,
            ensure_event_processing: true,
        })
    }

    /// Sets up the test environment with the given domain plugins registered.
    ///
    /// Each plugin's schema is initialised and an `EventDispatcher` is created
    /// and attached to the event processor runner so plugin events are routed.
    pub async fn setup_with_plugins(plugins: Vec<Arc<dyn NexusPlugin>>) -> Result<Self> {
        let (testnet, homeserver_id, pubky_id) = Self::init_stack_and_homeserver().await?;

        for plugin in &plugins {
            let ctx = PluginContext::for_plugin(plugin.as_ref());
            plugin.setup_schema(&ctx).await.map_err(|e| {
                Error::msg(format!(
                    "plugin '{}' schema setup failed: {e}",
                    plugin.manifest().name
                ))
            })?;
        }

        let dispatcher = Arc::new(EventDispatcher::new(plugins));

        let mut event_processor_runner = Self::create_test_event_processor_runner(pubky_id);
        event_processor_runner.dispatcher = Some(dispatcher);

        Ok(Self {
            testnet,
            homeserver_id,
            event_processor_runner,
            ensure_event_processing: true,
        })
    }

    /// Disables automatic event processing after each write.
    pub fn remove_event_processing(mut self) -> Self {
        self.ensure_event_processing = false;
        self
    }

    /// Runs the event processor until the homeserver event stream is exhausted.
    pub async fn ensure_event_processing_complete(&mut self) -> Result<()> {
        if self.ensure_event_processing {
            self.event_processor_runner
                .build(self.homeserver_id.clone())
                .await
                .map_err(|e| anyhow!(e))?
                .run()
                .await
                .map_err(|e| anyhow!(e))?;
        }
        Ok(())
    }

    /// PUTs `object` at `hs_path` for `user_keypair` then drains events.
    pub async fn put<T>(
        &mut self,
        user_keypair: &Keypair,
        hs_path: &ResourcePath,
        object: T,
    ) -> Result<()>
    where
        T: serde::Serialize,
    {
        let pubky = PubkyConnector::get()?;
        let signer = pubky.signer(user_keypair.clone());
        let session = signer.signin().await?;
        session
            .storage()
            .put(hs_path, serde_json::to_string(&object)?)
            .await?;
        self.ensure_event_processing_complete().await?;
        Ok(())
    }

    /// DELETEs `hs_path` for `user_keypair` then drains events.
    pub async fn del(&mut self, user_keypair: &Keypair, hs_path: &ResourcePath) -> Result<()> {
        let pubky = PubkyConnector::get()?;
        let signer = pubky.signer(user_keypair.clone());
        let session = signer.signin().await?;
        session.storage().delete(hs_path).await?;
        self.ensure_event_processing_complete().await?;
        Ok(())
    }

    /// Registers `user_kp` on the shared homeserver (409 = already registered, treated as OK).
    pub async fn register_user(&self, user_kp: &Keypair) -> Result<()> {
        let pubky = PubkyConnector::get()?;
        let signer = pubky.signer(user_kp.clone());
        let hs_pk: PublicKey = self.homeserver_id.clone().try_into()?;
        match signer.signup(&hs_pk, None).await {
            Ok(_) => {}
            Err(e) if e.to_string().contains("409") || e.to_string().contains("already exists") => {
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Registers `user_kp` on the specified homeserver (no 409 tolerance).
    pub async fn register_user_in_hs(&self, user_kp: &Keypair, hs_pk: &PublicKey) -> Result<()> {
        let pubky = PubkyConnector::get()?;
        let signer = pubky.signer(user_kp.clone());
        signer.signup(hs_pk, None).await?;
        Ok(())
    }

    /// Registers and writes a `PubkyAppUser` profile, returns the user's z32 ID.
    pub async fn create_user(&mut self, user_kp: &Keypair, user: &PubkyAppUser) -> Result<String> {
        let user_id = user_kp.public_key().to_z32();
        self.register_user(user_kp).await?;
        let user_path = PubkyAppUser::hs_path();
        self.put(user_kp, &user_path, user).await?;
        Ok(user_id)
    }

    /// Updates an existing user's profile (no signup, to avoid 412 on second signup).
    pub async fn create_profile(
        &mut self,
        user_kp: &Keypair,
        user: &PubkyAppUser,
    ) -> Result<String> {
        let user_id = user_kp.public_key().to_z32();
        let user_path = PubkyAppUser::hs_path();
        self.put(user_kp, &user_path, user).await?;
        Ok(user_id.to_string())
    }

    /// Creates a post with a unique timestamped ID, returns `(post_id, post_path)`.
    pub async fn create_post(
        &mut self,
        user_kp: &Keypair,
        post: &PubkyAppPost,
    ) -> Result<(String, ResourcePath)> {
        let post_id = generate_post_id();
        let post_path: ResourcePath = PubkyAppPost::create_path(&post_id).parse()?;
        self.put(user_kp, &post_path, post).await?;
        Ok((post_id, post_path))
    }

    /// Deletes the user profile record.
    pub async fn cleanup_user(&mut self, user_kp: &Keypair) -> Result<()> {
        let user_path = PubkyAppUser::hs_path();
        self.del(user_kp, &user_path).await
    }

    /// Deletes a post at the given path.
    pub async fn cleanup_post(
        &mut self,
        user_kp: &Keypair,
        post_path: &ResourcePath,
    ) -> Result<()> {
        self.del(user_kp, post_path).await
    }

    /// Creates a file record, returns `(file_id, file_path)`.
    pub async fn create_file(
        &mut self,
        user_kp: &Keypair,
        file: &PubkyAppFile,
    ) -> Result<(String, ResourcePath)> {
        let file_id = file.create_id();
        let file_path: ResourcePath = PubkyAppFile::create_path(&file_id).parse()?;
        self.put(user_kp, &file_path, file).await?;
        Ok((file_id, file_path))
    }

    /// Writes raw bytes to the homeserver (for file body uploads).
    pub async fn create_file_from_body(
        &mut self,
        user_kp: &Keypair,
        homeserver_uri: &str,
        object: Vec<u8>,
    ) -> Result<()> {
        let pubky = PubkyConnector::get()?;
        let signer = pubky.signer(user_kp.clone());
        let session = signer.signin().await?;
        session.storage().put(homeserver_uri, object).await?;
        Ok(())
    }

    /// Deletes a file at the given path.
    pub async fn cleanup_file(
        &mut self,
        user_kp: &Keypair,
        file_path: &ResourcePath,
    ) -> Result<()> {
        self.del(user_kp, file_path).await
    }

    /// Creates a follow relationship from `follower_kp` to `followee_id`.
    pub async fn create_follow(
        &mut self,
        follower_kp: &Keypair,
        followee_id: &str,
    ) -> Result<ResourcePath> {
        let follow_relationship = PubkyAppFollow {
            created_at: Utc::now().timestamp_millis(),
        };
        let follow_path = follow_relationship.hs_path(followee_id);
        self.put(follower_kp, &follow_path, follow_relationship)
            .await?;
        Ok(follow_path)
    }
}

// ── Convenience traits for building homeserver ResourcePaths from spec types ──

pub trait HomeserverIdPath: HasIdPath {
    fn hs_path(pubky_id: &str) -> ResourcePath {
        Self::create_path(pubky_id).parse().unwrap()
    }
}
impl<T> HomeserverIdPath for T where T: HasIdPath {}

pub trait HomeserverPath: HasPath {
    fn hs_path() -> ResourcePath {
        Self::create_path().parse().unwrap()
    }
}
impl<T> HomeserverPath for T where T: HasPath {}

pub trait HomeserverHashIdPath: HashId + HasIdPath {
    fn hs_path(&self) -> ResourcePath {
        let id = self.create_id();
        Self::create_path(&id).parse().unwrap()
    }
}
impl<T> HomeserverHashIdPath for T where T: HashId + HasIdPath {}

pub trait HomeserverPathForPubkyId {
    fn hs_path(&self, pubky_id: &str) -> ResourcePath;
}
impl HomeserverPathForPubkyId for PubkyAppFollow {
    fn hs_path(&self, pubky_id: &str) -> ResourcePath {
        Self::create_path(pubky_id).parse().unwrap()
    }
}
