//! Filesystem operations for the Disktree TSX application.
mod scanner;

use nsh_core::prelude::*;
use serde_json::Value;

/// Scans, browses and removes entries using the native disk index.
#[module]
pub struct DiskTree;

#[module]
impl DiskTree {
    /// Runs a filesystem operation with a JSON payload and returns its JSON result.
    pub async fn request(route: String, body: String) -> Result<String, String> {
        let data: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
        let response = match route.as_str() {
            "/scan" => {
                let progress = scanner::begin_scan(&data)?;
                blocking(move || scanner::run_scan(&data, progress)).await
            }
            // Unattended smoke tests drive the packaged app through this script.
            "/ui-script" => {
                let script = std::env::var("DISKTREE_UI_SCRIPT").unwrap_or_default();
                return Ok(Value::String(script).to_string());
            }
            // Records an update smoke outcome where scripts/smoke-updater.py reads it.
            "/smoke" => return scanner::record_smoke(&data).map(|()| "null".into()),
            "/cancel" | "/progress" | "/scan-progress" => {
                return scanner::dispatch(&route, &data).map(|result| result.to_string());
            }
            _ => blocking(move || scanner::dispatch(&route, &data)).await,
        };
        response
            .map_err(|error| error.to_string())?
            .map(|result| result.to_string())
    }
}

#[native_module("disktree-native")]
fn native(registry: &mut Registry) {
    registry.module::<DiskTree>();
}
