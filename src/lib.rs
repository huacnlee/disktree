//! Native Rust disk explorer: UI, interaction, geometry, and filesystem work.
mod model;
mod scanner;
mod state;
mod ui;
use nsh_core::prelude::*;
pub use ui::App;

#[native_module("disktree/native")]
fn native(_registry: &mut Registry) {}
