use std::{fmt::Debug, path::PathBuf, sync::Arc};

use axum::Router;
#[cfg(feature = "mapky")]
use mapky_nexus_plugin::MapkyPlugin;
use nexus_common::plugin::NexusPlugin;
#[cfg(feature = "mapky")]
use nexus_common::plugin::PluginContext;
use nexus_common::DaemonConfig;
use nexus_common::{types::DynError, utils::create_shutdown_rx};
use nexus_watcher::NexusWatcherBuilder;
use nexus_webapi::{api_context::ApiContextBuilder, NexusApiBuilder};
use serde::{Deserialize, Serialize};
use tokio::{sync::watch::Receiver, try_join};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonLauncher {}

impl DaemonLauncher {
    /// Starts a daemon, with separate threads for a [NexusApi] and a [NexusWatcher] instances.
    ///
    /// This is a blocking method. It only returns:
    /// - either when one of these services throws an error, or
    /// - when the shutdown signal is received and both services shut down
    ///
    /// ### Arguments
    ///
    /// - `config_dir`: the directory where the config file is expected to be
    /// - `shutdown_rx`: optional shutdown signal. If none is provided, a default one will be created, listening for Ctrl-C.
    pub async fn start(
        config_dir: PathBuf,
        shutdown_rx: Option<Receiver<bool>>,
    ) -> Result<(), DynError> {
        let shutdown_rx = shutdown_rx.unwrap_or_else(create_shutdown_rx);

        let config = DaemonConfig::read_or_create_config_file(config_dir.clone()).await?;

        let api_context = ApiContextBuilder::from_config_dir(config_dir)
            .try_build()
            .await?;

        // ── Domain plugins ──────────────────────────────────────────────
        // Plugins are compiled in via feature flags. Without a feature flag
        // nexusd builds and runs with no domain plugins (pure social graph).
        // Example: cargo run -p nexusd --features mapky

        #[allow(unused_mut)]
        let mut plugins: Vec<Arc<dyn NexusPlugin>> = vec![];
        #[allow(unused_mut)]
        let mut extra_routes = Router::new();

        #[cfg(feature = "mapky")]
        {
            let mapky: Arc<MapkyPlugin> = Arc::new(MapkyPlugin::new());
            let mapky_ctx = PluginContext {
                redis_prefix: "mapky".to_string(),
            };
            extra_routes = extra_routes.nest("/v0/mapky", mapky.routes(mapky_ctx));
            plugins.push(mapky);
        }

        // ── Watcher ─────────────────────────────────────────────────────
        let nexus_watcher_builder =
            NexusWatcherBuilder::with_stack(config.watcher, &config.stack)
                .with_plugins(plugins);

        // ── Webapi ──────────────────────────────────────────────────────
        #[allow(unused_mut)]
        let mut nexus_webapi_builder = NexusApiBuilder::new(api_context)
            .with_extra_routes(extra_routes);

        #[cfg(feature = "mapky")]
        {
            let mapky_docs = MapkyPlugin::new().openapi_docs();
            nexus_webapi_builder =
                nexus_webapi_builder.with_swagger_doc("/api-docs/mapky/openapi.json", mapky_docs);
        }

        try_join!(
            nexus_webapi_builder.start(Some(shutdown_rx.clone())),
            nexus_watcher_builder.start(Some(shutdown_rx))
        )?;
        Ok(())
    }
}
