//! Runs the production JSX against the real in-process Rust scanner.
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
// NSH writes step modules under the app’s shared .nsh/test directory.
static UI_SESSION: Mutex<()> = Mutex::new(());

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "requires nsh; run through scripts/nsh_toolchain.py"]
fn jsx_uses_native_scanning_navigation_and_refresh() {
    let _session = UI_SESSION.lock().unwrap_or_else(|error| error.into_inner());
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
    if let Some(cargo) = std::env::var_os("NSH_CARGO") {
        command.env("CARGO", cargo);
    }
    command
        .current_dir(&repo)
        .args(["test", "App.tsx", "--trace"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("DISKTREE_ROOT", &tree)
        .env("DISKTREE_STATE", &state);
    let labels = [
        "Files",
        "Disk size",
        "Edit folder path",
        "Edit folder path",
        "victim.bin",
        "Mark / unmark",
        "Delete marked permanently",
        "Back to explorer",
        "Delete marked permanently",
        "Confirm permanent deletion",
        "folder",
        "Open folder",
        "Parent folder",
        "folder",
        "Open folder",
        "Parent folder",
        "keep.bin",
        "Mark / unmark",
        "Review marked items →",
        "Delete permanently",
        "Back to explorer",
        "Delete marked permanently",
        "Back to explorer",
        "Empty Trash",
        "Back to explorer",
        "keep.bin",
        "Mark / unmark",
        "folder",
        "Open folder",
        "Mark / unmark",
        "Review marked items →",
        "Delete permanently",
        "Confirm permanent deletion",
    ];
    let mut steps = String::from("await app.wait(400);\n");
    for label in labels {
        let selector = serde_json::to_string(&format!("label:{label}")).unwrap();
        steps.push_str(&format!(
            "await app.press({selector}); await app.wait(400);\n"
        ));
    }
    let mut child = command.spawn().expect("run nsh test");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(steps.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait for nsh test");
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
    for label in ["Files", "Disk size", "Open folder"] {
        assert!(
            log.split("\n── ").any(|section| {
                section.starts_with(&format!("press(label:{label}):"))
                    && section.contains("LayoutAnimation")
            }),
            "{label} must animate its treemap layout change"
        );
    }
    assert!(log.contains("Scanning folders"));
    let confirmation = log.find("Confirm permanent deletion").unwrap();
    assert!(
        !log[confirmation..].contains("Scanning folders"),
        "removal must not start another scan"
    );
    assert!(!log.contains("Updating disk usage…"));
    assert!(!victim.exists());
    assert!(!folder.exists());
    assert!(keep.exists());
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(saved["directory"], tree.to_str().unwrap());
    assert!(!log.to_lowercase().contains("panicked at"));
}

#[test]
#[ignore = "requires nsh"]
fn metric_switch_applies_one_treemap_layout() {
    let _session = UI_SESSION.lock().unwrap_or_else(|error| error.into_inner());
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap()
        .join(".cache/nsh-disktree-test-tmp");
    let fixture = Fixture(base.join(format!("metric-{}-{}", std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())));
    let tree = fixture.0.join("tree");
    fs::create_dir_all(&tree).unwrap();
    for i in 0..120 {
        fs::write(
            tree.join(format!("item-{i:03}.bin")),
            vec![0; (i + 1) * 128],
        )
        .unwrap();
    }
    let mut child = Command::new(std::env::var_os("NSH").unwrap_or_else(|| "nsh".into()))
        .current_dir(&repo)
        .args(["test", "App.tsx", "--trace"])
        .env("DISKTREE_ROOT", &tree)
        .env("DISKTREE_STATE", fixture.0.join("state.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run nsh test");
    child.stdin.take().unwrap().write_all(
        b"await app.wait(500); await app.press('label:Files'); await app.wait(500); await app.press('label:Disk size'); await app.wait(500);"
    ).unwrap();
    let output = child.wait_with_output().unwrap();
    let log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{log}");
    for label in ["Files", "Disk size"] {
        let layouts: usize = log
            .split("\n── ")
            .filter(|section| section.starts_with(&format!("press(label:{label}):")))
            .map(|section| section.matches("LayoutAnimation(LayoutAnimation").count())
            .sum();
        assert_eq!(
            layouts, 1,
            "{label} must apply its new metric and directory view together, without first relaying out stale data"
        );
    }
}
