//! Rust filesystem capabilities exposed to the JSX interface.
mod scanner;
use nsh_core::ui::prelude::*;

#[native_module("disktree/native")]
fn native(registry: &mut Registry) {
    registry.module::<Scanner>();
}

/// Scanning, bounded directory queries and reviewed removal operations.
pub struct Scanner;

#[module]
impl Scanner {
    /// Runs a filesystem operation off the UI thread and returns bounded JSON.
    pub async fn request(route: String, body: String) -> Result<String, String> {
        blocking(move || scanner::request(&route, &body))
            .await
            .map_err(|e| e.to_string())?
    }
}
