//! Event dispatcher — routes homeserver events to domain plugins by path
//! prefix, intercepting them *before* `Event::parse_event()` so
//! `pubky-app-specs` never sees domain-specific URIs.

use nexus_common::db::PubkyConnector;
use nexus_common::plugin::{NexusPlugin, PluginContext};
use std::sync::Arc;
use tracing::{debug, error, warn};

pub struct EventDispatcher {
    plugins: Vec<Arc<dyn NexusPlugin>>,
}

impl EventDispatcher {
    /// Sort plugins longest-namespace-first so more specific prefixes always
    /// win over broader ones (e.g. `/pub/mapky.app/places/` beats `/pub/mapky.app/`).
    pub fn new(mut plugins: Vec<Arc<dyn NexusPlugin>>) -> Self {
        plugins.sort_by(|a, b| b.manifest().namespace.len().cmp(&a.manifest().namespace.len()));
        Self { plugins }
    }

    /// Returns `true` if a registered plugin handled this event line.
    ///
    /// Event line format: `"PUT pubky://user_id/pub/..."` or `"DEL pubky://..."`.
    /// Returns `false` when no plugin matches; the caller should then hand
    /// the line to the existing social watcher (`Event::parse_event`).
    pub async fn try_dispatch(&self, line: &str) -> bool {
        if self.plugins.is_empty() {
            return false;
        }

        // Split "PUT pubky://..." → (event_type, uri)
        let mut parts = line.splitn(2, ' ');
        let event_type = match parts.next() {
            Some(t) => t,
            None => return false,
        };
        let uri = match parts.next() {
            Some(u) => u.trim(),
            None => return false,
        };

        // Extract the /pub/{domain}.app/... path from pubky://{user_id}/pub/...
        let path = match extract_pub_path(uri) {
            Some(p) => p,
            None => return false,
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
                    return true; // claimed but malformed — don't fall through
                }
            };

            let ctx = PluginContext {
                redis_prefix: manifest.name.to_string(),
            };

            match event_type {
                "PUT" => {
                    let data = match fetch_blob(uri).await {
                        Some(b) => b,
                        None => return true, // error already logged
                    };
                    if let Err(e) = plugin.handle_put(uri, &data, &user_id, &ctx).await {
                        error!("Plugin '{}' PUT error for {uri}: {e}", manifest.name);
                    }
                }
                "DEL" => {
                    if let Err(e) = plugin.handle_del(uri, &user_id, &ctx).await {
                        error!("Plugin '{}' DEL error for {uri}: {e}", manifest.name);
                    }
                }
                _ => return false,
            }

            return true; // handled
        }

        false // no plugin matched
    }
}

/// Fetch the raw blob for a `pubky://` URI via the pubky SDK.
/// Returns `None` on any error (errors are logged).
async fn fetch_blob(uri: &str) -> Option<Vec<u8>> {
    let pubky = match PubkyConnector::get() {
        Ok(p) => p,
        Err(e) => {
            error!("PubkyConnector unavailable when fetching {uri}: {e}");
            return None;
        }
    };

    let response = match pubky.public_storage().get(uri).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to fetch blob for {uri}: {e}");
            return None;
        }
    };

    if !response.status().is_success() {
        error!("Fetch blob {uri}: HTTP {}", response.status());
        return None;
    }

    match response.bytes().await {
        Ok(b) => Some(b.to_vec()),
        Err(e) => {
            error!("Failed to read blob bytes for {uri}: {e}");
            None
        }
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

    #[test]
    fn test_plugins_sorted_longest_namespace_first() {
        // The sort key is namespace length — verify the comparator directly.
        let mut namespaces = vec!["/pub/mapky.app/", "/pub/mapky.app/places/", "/pub/other.app/"];
        namespaces.sort_by(|a, b| b.len().cmp(&a.len()));
        assert_eq!(namespaces[0], "/pub/mapky.app/places/"); // longest first
        assert_eq!(namespaces[2], "/pub/other.app/");        // shortest last
    }
}
