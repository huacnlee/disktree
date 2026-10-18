use super::*;
use std::io::Write;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = home().join(".cache/nsh-disktree-test-tmp");
        fs::create_dir_all(&base).unwrap();
        let path = base.join(format!(
            "rust-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, vec![1; 8192]).unwrap();
        p
    }
    fn scan(&self) -> Arc<Index> {
        scan(&self.0).unwrap().0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn query(index: &Index, path: &Path) -> Value {
    view(index, &json!({"path":path})).unwrap()
}
#[test]
fn allocated_size_hardlinks_and_symlinks() {
    let f = Fixture::new();
    let file = f.file("folder/sample.bin");
    fs::hard_link(&file, f.0.join("hardlink")).unwrap();
    std::os::unix::fs::symlink(f.0.join("folder"), f.0.join("link")).unwrap();
    let index = f.scan();
    assert_eq!(index.root.apparent, 8192);
    assert_eq!(index.root.files, 2);
    assert_eq!(index.root.size, fs::metadata(file).unwrap().blocks() * 512);
    assert!(scan(&f.0.join("link")).is_err());
}
#[test]
fn view_pages_search_metrics_and_totals() {
    let f = Fixture::new();
    for i in 0..100 {
        f.file(&format!("file-{i:03}"));
    }
    f.file(".hidden");
    let index = f.scan();
    let first = query(&index, &f.0);
    assert_eq!(first["children"].as_array().unwrap().len(), 80);
    assert_eq!(first["count"], 101);
    assert_eq!(first["node"]["children"], json!([]));
    assert_eq!(
        first["map"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["size"].as_u64().unwrap())
            .sum::<u64>(),
        index.root.size
    );
    let second = view(&index, &json!({"path":f.0,"offset":80,"metric":"files"})).unwrap();
    assert_eq!(second["children"].as_array().unwrap().len(), 21);
    assert_eq!(
        view(&index, &json!({"path":f.0,"query":"file-099"})).unwrap()["count"],
        1
    );
    assert_eq!(
        view(&index, &json!({"path":f.0,"hidden":false})).unwrap()["count"],
        100
    );
    assert!(view(&index, &json!({"path":f.0,"metric":"invalid"})).is_err());
}
#[test]
fn nested_map_has_bounded_payload_and_preserves_weight() {
    let f = Fixture::new();
    for i in 0..400 {
        f.file(&format!("folder/file-{i:03}"));
    }
    let index = f.scan();
    let v = query(&index, &f.0);
    fn count(nodes: &Value) -> usize {
        nodes
            .as_array()
            .unwrap()
            .iter()
            .map(|n| 1 + count(&n["children"]))
            .sum()
    }
    assert!(count(&v["map"]) <= MAP_BUDGET + 1);
    assert_eq!(
        v["map"][0]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["size"].as_u64().unwrap())
            .sum::<u64>(),
        index.root.size
    );
    assert!(serde_json::to_string(&v).unwrap().len() < 100_000);
}
#[test]
fn removal_rejects_root_outside_overlaps_and_changed_files() {
    let f = Fixture::new();
    let file = f.file("folder/file");
    let index = f.scan();
    for paths in [
        vec![f.0.to_string_lossy().into_owned()],
        vec!["/etc/passwd".into()],
        vec![
            f.0.join("folder").to_string_lossy().into_owned(),
            file.to_string_lossy().into_owned(),
        ],
    ] {
        assert!(validate_removal(&index, &paths, true).is_err());
    }
    fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert!(validate_removal(&index, &[file.to_string_lossy().into_owned()], true).is_err());
    assert!(
        validate_removal(
            &index,
            &[f.0.join("folder").to_string_lossy().into_owned()],
            true
        )
        .is_err()
    );
}
#[test]
fn replaced_ancestor_cannot_redirect_removal() {
    let f = Fixture::new();
    let file = f.file("folder/file");
    let index = f.scan();
    fs::rename(f.0.join("folder"), f.0.join("moved")).unwrap();
    std::os::unix::fs::symlink(f.0.join("moved"), f.0.join("folder")).unwrap();
    assert!(validate_removal(&index, &[file.to_string_lossy().into_owned()], true).is_err());
}
#[test]
fn permanent_delete_unlinks_symlink_without_touching_target() {
    let f = Fixture::new();
    let file = f.file("retained");
    let link = f.0.join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let index = f.scan();
    assert_eq!(
        remove(&index, &json!({"paths":[link]}), true).unwrap()["completed"],
        json!([link])
    );
    assert!(file.exists());
    assert!(fs::symlink_metadata(link).is_err());
}
#[test]
fn deleting_current_directory_refreshes_to_surviving_parent() {
    let f = Fixture::new();
    f.file("folder/file");
    let index = f.scan();
    remove(&index, &json!({"paths":[f.0.join("folder")]}), true).unwrap();
    let fresh = f.scan();
    let v = view(&fresh, &json!({"path":f.0.join("folder"),"fallback":true})).unwrap();
    assert_eq!(v["node"]["path"], f.0.to_str().unwrap());
    assert_eq!(v["count"], 0);
    assert_eq!(fresh.root.files, 0);
    assert!(view(&fresh, &json!({"path":f.0.parent(),"fallback":true})).is_err());
}
#[test]
fn state_persistence_and_missing_locations() {
    let f = Fixture::new();
    let state = f.0.join("preferences/state.json");
    assert_eq!(saved_location(&state), home());
    save_location(f.0.to_str().unwrap(), &state).unwrap();
    assert_eq!(saved_location(&state), f.0);
    save_location(f.0.join("missing").to_str().unwrap(), &state).unwrap();
    assert_eq!(saved_location(&state), home());
    fs::write(&state, b"not JSON").unwrap();
    assert_eq!(saved_location(&state), home());
}
#[test]
fn system_trash_commands_keep_paths_separate_from_code() {
    let path = "/a file/with ' quotes";
    let cmd = trash_command(path);
    assert_eq!(cmd.get_args().last().unwrap(), path);
    let empty = empty_command();
    let args: Vec<_> = empty.get_args().collect();
    if cfg!(target_os = "macos") {
        assert_eq!(
            args,
            vec!["-e", "tell application \"Finder\" to empty trash"]
        );
    } else {
        assert_eq!(args, vec!["trash", "--empty"]);
    }
}

#[test]
fn permanent_removal_rejects_unscanned_mounted_descendants() {
    let f = Fixture::new();
    let file = f.file("folder/file");
    let mut index = f.scan();
    Arc::get_mut(&mut index)
        .unwrap()
        .records
        .get_mut(file.to_str().unwrap())
        .unwrap()
        .device += 1;
    let error = validate_removal(
        &index,
        &[f.0.join("folder").to_string_lossy().into_owned()],
        true,
    )
    .unwrap_err();
    assert!(error.contains("mounted volumes"));
    assert!(file.exists());
}
#[test]
fn removal_deduplicates_selected_paths() {
    let f = Fixture::new();
    let file = f.file("file");
    let index = f.scan();
    assert_eq!(
        remove(&index, &json!({"paths":[file,file]}), true).unwrap()["completed"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!file.exists());
}
#[test]
fn invalid_requests_and_failed_system_commands_are_reported() {
    assert!(request("/unknown", "{}").is_err());
    assert!(request("/scan", "invalid JSON").is_err());
    assert!(index(&json!({"id":"expired"})).is_err());
    assert!(execute(&mut Command::new("/nonexistent-disktree-command")).is_err());
}
#[test]
#[ignore = "allocates a 650,000-file native index; run as a release gate"]
fn large_index_keeps_query_payload_bounded() {
    let count = 650_000;
    let files: Vec<_> = (0..count)
        .map(|i| {
            Arc::new(Node {
                path: format!(
                    "/fixture/archive/{}/file-{i}",
                    "long-directory-name/".repeat(5)
                ),
                name: format!("file-{i:06}"),
                directory: false,
                size: 4096,
                apparent: 4000,
                files: 1,
                modified: 1.0,
                children: vec![],
            })
        })
        .collect();
    let folder = Arc::new(Node {
        path: "/fixture/archive".into(),
        name: "archive".into(),
        directory: true,
        size: count * 4096,
        apparent: count * 4000,
        files: count,
        modified: 1.0,
        children: files,
    });
    let root = Arc::new(Node {
        path: "/fixture".into(),
        name: "fixture".into(),
        directory: true,
        size: folder.size,
        apparent: folder.apparent,
        files: count,
        modified: 1.0,
        children: vec![folder.clone()],
    });
    let index = Index {
        id: "stress".into(),
        nodes: HashMap::from([
            (root.path.clone(), root.clone()),
            (folder.path.clone(), folder.clone()),
        ]),
        root,
        records: HashMap::new(),
    };
    for (offset, metric) in [(0, "size"), (80, "files"), (160, "apparent")] {
        let result = view(
            &index,
            &json!({"path":folder.path,"offset":offset,"metric":metric,"levels":3}),
        )
        .unwrap();
        let length = serde_json::to_string(&result).unwrap().len();
        assert!(length < 100_000);
        assert_eq!(result["children"].as_array().unwrap().len(), PAGE_SIZE);
        assert_eq!(
            result["map"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["size"].as_u64().unwrap())
                .sum::<u64>(),
            folder.size
        );
    }
    assert_eq!(
        view(&index, &json!({"path":folder.path,"query":"file-649999"})).unwrap()["count"],
        1
    );
}
