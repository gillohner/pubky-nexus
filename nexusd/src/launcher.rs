use std::{fmt::Debug, path::PathBuf, sync::Arc};

use axum::Router;
use mapky_nexus_plugin::MapkyPlugin;
use nexus_common::plugin::{NexusPlugin, PluginContext};
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
        // Each plugin is shared between the watcher (event indexing) and the
        // webapi (API routes). The watcher builder calls setup_schema() on
        // startup; the webapi builder mounts the plugin's routes.

        let mapky: Arc<MapkyPlugin> = Arc::new(MapkyPlugin::new());
        let mapky_ctx = PluginContext {
            redis_prefix: "mapky".to_string(),
        };
        let mapky_routes = Router::new().nest("/v0/mapky", mapky.routes(mapky_ctx));
        let mapky_docs = mapky.openapi_docs();

        // ── Watcher ─────────────────────────────────────────────────────
        let nexus_watcher_builder =
            NexusWatcherBuilder::with_stack(config.watcher, &config.stack)
                .with_plugins(vec![mapky]);

        // ── Webapi ──────────────────────────────────────────────────────
        let nexus_webapi_builder = NexusApiBuilder::new(api_context)
            .with_extra_routes(mapky_routes)
            .with_swagger_doc("/api-docs/mapky/openapi.json", mapky_docs);

        try_join!(
            nexus_webapi_builder.start(Some(shutdown_rx.clone())),
            nexus_watcher_builder.start(Some(shutdown_rx))
        )?;
        Ok(())
    }
}
