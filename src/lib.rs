//! Native Rust disk explorer: UI, interaction, geometry, and filesystem work.
mod model;
mod scanner;
mod state;
mod ui;
mod updater;
use nsh_core::prelude::*;
pub use ui::App;

#[native_module("disktree/native")]
fn native(_registry: &mut Registry) {}

/// Leave the standalone native event loop after its current UI batch.
pub(crate) fn quit() {
    #[cfg(target_os = "macos")]
    nsh_platform_macos::quit_app();
    #[cfg(target_os = "linux")]
    nsh_core::element::desktop::close_window();
}
