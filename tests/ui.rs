//! Drives the production Rust root and real scanner without a JS runtime.
use nsh_core::{
    MountItem,
    geometry::{px, size},
    ui::Root,
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn rust_ui_scans_browses_filters_and_confirms_removal() {
    let f = Fixture(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".nsh/test-data")
            .join(format!(
                "disktree-rust-ui-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
    );
    let tree = f.0.join("tree");
    fs::create_dir_all(tree.join("folder")).unwrap();
    fs::write(tree.join("folder/nested.bin"), vec![1; 4096]).unwrap();
    fs::write(tree.join("victim.bin"), vec![2; 8192]).unwrap();
    fs::write(tree.join("keep.bin"), vec![3; 4096]).unwrap();
    // The integration test is the sole test in its process; set before the engine starts.
    unsafe {
        std::env::set_var("DISKTREE_ROOT", &tree);
        std::env::set_var("DISKTREE_STATE", f.0.join("state.json"));
    }
    let mut ui = Headless::start(EngineConfig {
        viewport: size(px(1280.), px(850.)),
        ..EngineConfig::new(Root::of::<disktree::App>())
    })
    .unwrap();
    ui.wait_for("label:Select victim.bin").unwrap();
    assert!(!ui.screen().matches("label:Review marked items →").unwrap());
    assert!(
        !ui.screen()
            .matches("label:Delete marked permanently")
            .unwrap()
    );
    assert!(!ui.screen().matches("label:Empty Trash…").unwrap());
    assert!(!ui.screen().matches("label:Next ›").unwrap());
    let refresh = ui
        .screen()
        .press_target("label:Scan folder")
        .unwrap()
        .unwrap();
    let flashed = Arc::new(AtomicBool::new(false));
    let observed = flashed.clone();
    ui.observe(move |_, batch| {
        for item in &batch.items {
            if let MountItem::ViewStyle { id, style } = item
                && *id == refresh
                && style.opacity < 1.0
            {
                observed.store(true, Ordering::Relaxed);
            }
        }
    });
    for metric in ["Files", "Apparent", "Disk size"] {
        ui.press(&format!("label:{metric}")).unwrap();
        ui.wait_for("label:Select victim.bin").unwrap();
    }
    assert!(
        !flashed.load(Ordering::Relaxed),
        "metric changes dimmed Refresh"
    );
    ui.press("label:View options").unwrap();
    ui.wait_for("label:Empty Trash…").unwrap();
    ui.press("label:Close view options").unwrap();
    assert!(!ui.screen().matches("label:Empty Trash…").unwrap());
    ui.press("label:View options").unwrap();
    ui.press("label:Empty Trash…").unwrap();
    ui.wait_for("label:Confirm empty Trash").unwrap();
    ui.press("label:Back to explorer").unwrap();
    ui.press("label:Select victim.bin").unwrap();
    ui.press("label:Mark / unmark").unwrap();
    ui.press("label:Review marked items →").unwrap();
    ui.wait_for("text:Move these items to Trash?").unwrap();
    assert!(tree.join("victim.bin").exists());
    ui.press("label:Delete permanently").unwrap();
    ui.wait_for("label:Confirm permanent deletion").unwrap();
    assert!(tree.join("victim.bin").exists());
    ui.press("label:Confirm permanent deletion").unwrap();
    ui.wait_until("victim removed", Duration::from_secs(5), |screen| {
        !tree.join("victim.bin").exists() && screen.matches("label:Select keep.bin").unwrap()
    })
    .unwrap();
    assert!(tree.join("keep.bin").exists());
    ui.press("label:Select folder").unwrap();
    ui.press("label:Open folder").unwrap();
    ui.wait_for("label:Select nested.bin").unwrap();
    ui.press("label:Parent folder").unwrap();
    ui.wait_for("label:Select keep.bin").unwrap();
    ui.type_text("label:Filter names", "keep").unwrap();
    ui.wait_for_gone("label:Select folder", Duration::from_secs(5))
        .unwrap();
    ui.wait_for("label:Select keep.bin").unwrap();
}
