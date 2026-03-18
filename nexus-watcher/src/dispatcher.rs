//! Event dispatcher — routes homeserver events to domain plugins by path
//! prefix, intercepting them *before* `Event::parse_event()` so
//! `pubky-app-specs` never sees domain-specific URIs.

use nexus_common::models::event::EventProcessorError;
use nexus_common::plugin::{NexusPlugin, PluginContext};
use std::sync::Arc;
use tracing::{debug, warn};

pub struct EventDispatcher {
    plugins: Vec<Arc<dyn NexusPlugin>>,
}

impl std::fmt::Debug for EventDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventDispatcher")
            .field("plugins", &self.plugins.iter().map(|p| p.manifest().name).collect::<Vec<_>>())
            .finish()
    }
}

impl EventDispatcher {
    /// Sort plugins longest-namespace-first so more specific prefixes always
    /// win over broader ones (e.g. `/pub/mapky.app/places/` beats `/pub/mapky.app/`).
    pub fn new(mut plugins: Vec<Arc<dyn NexusPlugin>>) -> Self {
        plugins.sort_by(|a, b| b.manifest().namespace.len().cmp(&a.manifest().namespace.len()));
        Self { plugins }
    }

    /// Returns `Ok(true)` if a registered plugin handled this event line,
    /// `Ok(false)` if no plugin matched (caller should fall through to social
    /// watcher), or `Err` if a plugin claimed the event but processing failed
    /// (caller should push to retry queue).
    ///
    /// Event line format: `"PUT pubky://user_id/pub/..."` or `"DEL pubky://..."`.
    pub async fn try_dispatch(&self, line: &str) -> Result<bool, EventProcessorError> {
        if self.plugins.is_empty() {
            return Ok(false);
        }

        // Split "PUT pubky://..." → (event_type, uri)
        let mut parts = line.splitn(2, ' ');
        let event_type = match parts.next() {
            Some(t) => t,
            None => return Ok(false),
        };
        let uri = match parts.next() {
            Some(u) => u.trim(),
            None => return Ok(false),
        };

        // Extract the /pub/{domain}.app/... path from pubky://{user_id}/pub/...
        let path = match extract_pub_path(uri) {
            Some(p) => p,
            None => return Ok(false),
        };

        for plugin in &self.plugins {
            let manifest = plugin.manifest();
            if !path.starts_with(manifest.namespace) {
                continue;
            }

            debug!("Plugin '{}' handling {} {uri}", manifest.name, event_type);

            let user_id = match extract_user_id(uri) {
                Some(u) => u,
                None => {
                    warn!("Could not extract user_id from URI: {uri}");
                    return Ok(true); // claimed but malformed — don't fall through
                }
            };

            let ctx = PluginContext::for_plugin(plugin.as_ref());

            match event_type {
                "PUT" => {
                    let data = nexus_common::db::fetch_blob(uri).await?;
                    plugin
                        .handle_put(uri, &data, &user_id, &ctx)
                        .await
                        .map_err(EventProcessorError::generic)?;
                }
                "DEL" => {
                    plugin
                        .handle_del(uri, &user_id, &ctx)
                        .await
                        .map_err(EventProcessorError::generic)?;
                }
                _ => return Ok(false),
            }

            return Ok(true); // handled
        }

        Ok(false) // no plugin matched
    }
}

/// Extract `/pub/{domain}.app/...` from `pubky://{user_id}/pub/...`.
fn extract_pub_path(uri: &str) -> Option<&str> {
    let without_scheme = uri.strip_prefix("pubky://")?;
    let slash_pos = without_scheme.find('/')?;
    let path = &without_scheme[slash_pos..];
    if path.starts_with("/pub/") {
        Some(path)
    } else {
        None
    }
}

/// Extract `user_id` from `pubky://{user_id}/pub/...`.
fn extract_user_id(uri: &str) -> Option<String> {
    let without_scheme = uri.strip_prefix("pubky://")?;
    let slash_pos = without_scheme.find('/')?;
    Some(without_scheme[..slash_pos].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pub_path() {
        let uri = "pubky://abc123/pub/mapky.app/posts/0034TK01CC73G";
        assert_eq!(
            extract_pub_path(uri),
            Some("/pub/mapky.app/posts/0034TK01CC73G")
        );
    }

    #[test]
    fn test_extract_user_id() {
        let uri = "pubky://abc123/pub/mapky.app/posts/0034TK01CC73G";
        assert_eq!(extract_user_id(uri), Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_pub_path_non_pub() {
        assert_eq!(extract_pub_path("pubky://abc123/other/path"), None);
    }

    #[test]
    fn test_dispatcher_empty() {
        let dispatcher = EventDispatcher::new(vec![]);
        assert!(dispatcher.plugins.is_empty());
    }

    // ── Integration tests with a minimal mock plugin ──────────────────────

    struct MockPlugin;

    #[async_trait::async_trait]
    impl NexusPlugin for MockPlugin {
        fn manifest(&self) -> nexus_common::plugin::PluginManifest {
            nexus_common::plugin::PluginManifest {
                name: "mock",
                namespace: "/pub/mock.app/",
            }
        }
        async fn handle_put(&self, _: &str, _: &[u8], _: &str, _: &PluginContext) -> Result<(), nexus_common::types::DynError> {
            Ok(())
        }
        async fn handle_del(&self, _: &str, _: &str, _: &PluginContext) -> Result<(), nexus_common::types::DynError> {
            Ok(())
        }
        fn routes(&self, _: PluginContext) -> axum::Router {
            axum::Router::new()
        }
        async fn setup_schema(&self, _: &PluginContext) -> Result<(), nexus_common::types::DynError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_try_dispatch_matches_del_event() {
        let dispatcher = EventDispatcher::new(vec![Arc::new(MockPlugin) as Arc<dyn NexusPlugin>]);
        let result = dispatcher
            .try_dispatch("DEL pubky://abc123/pub/mock.app/items/id1")
            .await;
        assert!(matches!(result, Ok(true)));
    }

    #[tokio::test]
    async fn test_try_dispatch_no_match_returns_false() {
        let dispatcher = EventDispatcher::new(vec![Arc::new(MockPlugin) as Arc<dyn NexusPlugin>]);
        let result = dispatcher
            .try_dispatch("DEL pubky://abc123/pub/other.app/items/id1")
            .await;
        assert!(matches!(result, Ok(false)));
    }

    #[tokio::test]
    async fn test_try_dispatch_empty_dispatcher_returns_false() {
        let dispatcher = EventDispatcher::new(vec![]);
        let result = dispatcher
            .try_dispatch("DEL pubky://abc123/pub/mock.app/items/id1")
            .await;
        assert!(matches!(result, Ok(false)));
    }

    #[test]
    fn test_plugins_sorted_longest_namespace_first() {
        // The sort key is namespace length — verify the comparator directly.
        let mut namespaces = vec!["/pub/mapky.app/", "/pub/mapky.app/places/", "/pub/other.app/"];
        namespaces.sort_by(|a, b| b.len().cmp(&a.len()));
        assert_eq!(namespaces[0], "/pub/mapky.app/places/"); // longest first
        assert_eq!(namespaces[2], "/pub/other.app/");        // shortest last
    }
}
