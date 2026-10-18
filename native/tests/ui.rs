//! Runs the production JSX against the real in-process Rust scanner.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "requires nsh and NSH_SOURCE; run after preparing the nsh toolchain"]
fn jsx_uses_native_scanning_navigation_and_refresh() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap()
        .join(".cache/nsh-disktree-test-tmp");
    fs::create_dir_all(&base).unwrap();
    let f = Fixture(base.join(format!(
            "ui-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let tree = f.0.join("tree");
    let folder = tree.join("folder");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("nested.bin"), vec![1; 4096]).unwrap();
    let victim = tree.join("victim.bin");
    fs::write(&victim, vec![1; 8192]).unwrap();
    let keep = tree.join("keep.bin");
    fs::write(&keep, vec![1; 4096]).unwrap();
    let state = f.0.join("state.json");
    let mut command = Command::new(std::env::var_os("NSH").unwrap_or_else(|| "nsh".into()));
    command
        .current_dir(&repo)
        .args([
            "trace", "--width", "1300", "--height", "850", "--wait", "400",
        ])
        .env("DISKTREE_ROOT", &tree)
        .env("DISKTREE_STATE", &state);
    let root_label = format!("Go to {}", tree.display());
    let labels = [
        "Edit folder path",
        "Edit folder path",
        "victim.bin",
        "Delete permanently",
        "Back to explorer",
        "Delete permanently",
        "Confirm permanent deletion",
        "folder",
        "Open folder",
        &root_label,
        "folder",
        "Open folder",
        "Parent folder",
        "keep.bin",
        "Mark / unmark",
        "Review marked items →",
        "Back to explorer",
        "Delete marked permanently",
        "Back to explorer",
        "Empty Trash",
        "Back to explorer",
        "folder",
        "Open folder",
        "Delete permanently",
        "Confirm permanent deletion",
    ];
    for label in labels {
        command.args(["--tap", &format!("label:{label}")]);
    }
    let output = command.output().expect("run nsh trace");
    let log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::create_dir_all(repo.join("target/agents")).unwrap();
    fs::write(repo.join("target/agents/native-ui.log"), &log).unwrap();
    assert!(
        output.status.success(),
        "{}",
        &log[log.len().saturating_sub(4000)..]
    );
    assert!(log.contains("Scan complete"));
    assert!(log.contains("Updating disk usage…"));
    assert!(!victim.exists());
    assert!(!folder.exists());
    assert!(keep.exists());
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(saved["directory"], tree.to_str().unwrap());
    assert!(!log.to_lowercase().contains("panicked at"));
}
