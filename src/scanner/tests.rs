use super::*;
use std::io::Write;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".nsh/test-data");
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
    let mut index = f.scan();
    let id = index.id.clone();
    let response = remove(&index, &json!({"paths":[f.0.join("folder")]}), true).unwrap();
    let completed = response["completed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    prune_removed(Arc::make_mut(&mut index), &completed);
    let v = view(&index, &json!({"path":f.0.join("folder"),"fallback":true})).unwrap();
    assert_eq!(v["node"]["path"], f.0.to_str().unwrap());
    assert_eq!(v["count"], 0);
    assert_eq!(index.root.files, 0);
    assert_eq!(index.id, id);
    assert!(view(&index, &json!({"path":f.0.parent(),"fallback":true})).is_err());
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
    Arc::make_mut(
        Arc::make_mut(&mut index)
            .nodes
            .get_mut(file.to_str().unwrap())
            .unwrap(),
    )
    .record
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
                )
                .into(),
                name: format!("file-{i:06}"),
                record: Record {
                    device: 1,
                    inode: i,
                    modified: 1,
                    nanos: 0,
                    size: 4000,
                },
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
        record: Record {
            device: 1,
            inode: 0,
            modified: 1,
            nanos: 0,
            size: 0,
        },
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
        record: Record {
            device: 1,
            inode: 0,
            modified: 1,
            nanos: 0,
            size: 0,
        },
        directory: true,
        size: folder.size,
        apparent: folder.apparent,
        files: count,
        modified: 1.0,
        children: vec![folder.clone()],
    });
    let index = Index {
        id: "stress".into(),
        nodes: [
            (root.path.clone(), root.clone()),
            (folder.path.clone(), folder.clone()),
        ]
        .into_iter()
        .collect(),
        root,
        shared: HashSet::default(),
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

#[test]
fn disk_block_counts_widen_before_multiplication() {
    assert_eq!(block_bytes(u32::MAX, 4096), u64::from(u32::MAX) * 4096);
    assert_eq!(
        block_bytes(u64::from(u32::MAX) + 1, 4096),
        (u64::from(u32::MAX) + 1) * 4096
    );
}

#[test]
fn scan_progress_reports_directories_files_and_deduplicated_bytes() {
    let fixture = Fixture::new();
    let original = fixture.file("first/nested/data.bin");
    fs::hard_link(&original, fixture.0.join("alias.bin")).unwrap();
    std::os::unix::fs::symlink(&fixture.0, fixture.0.join("link")).unwrap();
    let progress = Arc::new(Mutex::new(Value::Null));
    let (index, _) = scan_with_progress(&fixture.0, Some(progress.clone())).unwrap();
    let value = progress.lock().unwrap();
    assert_eq!(value["directories"], 3);
    assert_eq!(value["files"], 2);
    assert_eq!(value["bytes"], index.root.size);
    assert_eq!(value["errors"], 0);
    let current = Path::new(value["path"].as_str().unwrap());
    assert!(current.is_dir());
    assert!(current.starts_with(&fixture.0));
    assert!(value.to_string().len() < 1024);
}

#[test]
#[ignore = "filesystem timing benchmark; set DISKTREE_BENCH_ROOT to an existing directory"]
fn scan_benchmark() {
    let fixture = Fixture::new();
    let root = match std::env::var_os("DISKTREE_BENCH_ROOT") {
        Some(path) => PathBuf::from(path),
        None => {
            for directory in 0..64 {
                let folder = fixture.0.join(format!("dir-{directory}"));
                fs::create_dir(&folder).unwrap();
                for file in 0..256 {
                    fs::write(folder.join(format!("file-{file}")), []).unwrap();
                }
            }
            fixture.0.clone()
        }
    };
    let mut serial = Vec::new();
    let mut parallel = Vec::new();
    for iteration in 0..4 {
        for workers in if iteration % 2 == 0 { [1, 4] } else { [4, 1] } {
            let started = Instant::now();
            let bulk = std::env::var_os("DISKTREE_BENCH_STANDARD").is_none();
            let (index, _) = scan_with_reader(&root, None, workers, bulk).unwrap();
            let elapsed = started.elapsed();
            eprintln!(
                "scan benchmark {iteration}, {workers} workers: {elapsed:?}, {} entries, {} files, {} bytes",
                index.nodes.len(),
                index.root.files,
                index.root.size
            );
            if iteration > 0 {
                if workers == 1 {
                    serial.push(elapsed);
                } else {
                    parallel.push(elapsed);
                }
            }
        }
    }
    serial.sort();
    parallel.sort();
    eprintln!(
        "median: serial {:?}, parallel {:?}, speedup {:.2}x",
        serial[1],
        parallel[1],
        serial[1].as_secs_f64() / parallel[1].as_secs_f64()
    );
}

#[test]
fn parallel_scan_preserves_index_and_removal_validation() {
    let fixture = Fixture::new();
    for directory in 0..160 {
        for file in 0..3 {
            fixture.file(&format!("branch-{directory}/nested/file-{file}"));
        }
    }
    let original = fixture.0.join("branch-0/nested/file-0");
    fs::hard_link(&original, fixture.0.join("branch-23/alias")).unwrap();
    std::os::unix::fs::symlink(&fixture.0, fixture.0.join("cycle")).unwrap();
    // Invalid UTF-8 must be reported and skipped, without abandoning siblings.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStringExt;
        fs::write(fixture.0.join(std::ffi::OsString::from_vec(vec![0xff])), []).unwrap();
    }
    let (serial, serial_response) = scan_with_workers(&fixture.0, None, 1).unwrap();
    for _ in 0..3 {
        let progress = Arc::new(Mutex::new(Value::Null));
        let (parallel, response) =
            scan_with_workers(&fixture.0, Some(progress.clone()), 4).unwrap();
        assert_eq!(parallel.root.size, serial.root.size);
        assert_eq!(parallel.root.apparent, serial.root.apparent);
        assert_eq!(parallel.root.files, serial.root.files);
        assert_eq!(
            parallel
                .nodes
                .iter()
                .map(|(p, n)| (p.clone(), n.record.clone()))
                .collect::<HashMap<_, _>>(),
            serial
                .nodes
                .iter()
                .map(|(p, n)| (p.clone(), n.record.clone()))
                .collect::<HashMap<_, _>>()
        );
        assert_eq!(parallel.nodes.len(), serial.nodes.len());
        assert_eq!(response["errorCount"], serial_response["errorCount"]);
        assert_eq!(
            response["errorCount"],
            usize::from(cfg!(target_os = "linux"))
        );
        assert_eq!(progress.lock().unwrap()["files"], parallel.root.files);
        assert!(
            parallel.nodes[fixture.0.join("cycle").to_str().unwrap()]
                .children
                .is_empty()
        );
        for node in parallel.nodes.values().filter(|node| node.directory) {
            assert_eq!(
                node.size,
                node.children.iter().map(|child| child.size).sum::<u64>()
            );
            assert!(
                node.children
                    .windows(2)
                    .all(|pair| pair[0].size >= pair[1].size)
            );
        }
        let victim = fixture.0.join("branch-1");
        validate_removal(&parallel, &[victim.to_string_lossy().into_owned()], true).unwrap();
    }
}

#[test]
#[cfg(target_os = "macos")]
fn bulk_and_stat_scans_preserve_exact_records_and_sizes() {
    let fixture = Fixture::new();
    for directory in 0..4 {
        for file in 0..140 {
            fixture.file(&format!("branch-{directory}/file-{file}"));
        }
    }
    let sparse = fixture.0.join("sparse");
    fs::File::create(&sparse)
        .unwrap()
        .set_len(16 << 20)
        .unwrap();
    fs::hard_link(&sparse, fixture.0.join("sparse-alias")).unwrap();
    std::os::unix::fs::symlink(&fixture.0, fixture.0.join("cycle")).unwrap();
    let precise = fixture.0.join("branch-0/file-0");
    fs::File::options()
        .write(true)
        .open(&precise)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789))
        .unwrap();
    let (standard, _) = scan_with_reader(&fixture.0, None, 4, false).unwrap();
    let (bulk, _) = scan_with_reader(&fixture.0, None, 4, true).unwrap();
    assert_eq!(
        standard
            .nodes
            .iter()
            .map(|(p, n)| (p.clone(), n.record.clone()))
            .collect::<HashMap<_, _>>(),
        bulk.nodes
            .iter()
            .map(|(p, n)| (p.clone(), n.record.clone()))
            .collect::<HashMap<_, _>>()
    );
    assert_eq!(
        bulk.nodes[precise.to_str().unwrap()].record.nanos,
        123_456_789
    );
    assert_eq!(standard.root.size, bulk.root.size);
    assert_eq!(standard.root.apparent, bulk.root.apparent);
    assert_eq!(standard.root.files, bulk.root.files);
    assert_eq!(standard.nodes.len(), bulk.nodes.len());
    validate_removal(&bulk, &[precise.to_string_lossy().into_owned()], true).unwrap();
    // A same-size replacement in the same second must still fail validation.
    fs::File::options()
        .write(true)
        .open(&precise)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::new(1_700_000_000, 987_654_321))
        .unwrap();
    assert!(validate_removal(&bulk, &[precise.to_string_lossy().into_owned()], true).is_err());
}

#[test]
fn removal_updates_only_affected_branches_and_allows_later_deletion() {
    let f = Fixture::new();
    let victim = f.file("folder/victim");
    let keep = f.file("folder/keep");
    f.file("untouched/keep");
    let mut index = f.scan();
    let original_id = index.id.clone();
    let untouched = index.nodes[f.0.join("untouched").to_str().unwrap()].clone();
    let expected = index.root.size - index.nodes[victim.to_str().unwrap()].size;
    remove(&index, &json!({"paths":[victim]}), true).unwrap();
    prune_removed(
        Arc::make_mut(&mut index),
        &[victim.to_string_lossy().into_owned()],
    );
    assert_eq!(index.id, original_id);
    assert_eq!(index.root.size, expected);
    assert_eq!(index.root.files, 2);
    assert!(!index.nodes.contains_key(victim.to_str().unwrap()));
    assert!(Arc::ptr_eq(
        &untouched,
        &index.nodes[f.0.join("untouched").to_str().unwrap()]
    ));
    validate_removal(
        &index,
        &[f.0.join("folder").to_string_lossy().into_owned()],
        true,
    )
    .unwrap();
    remove(&index, &json!({"paths":[keep]}), true).unwrap();
    prune_removed(
        Arc::make_mut(&mut index),
        &[keep.to_string_lossy().into_owned()],
    );
    assert_eq!(index.root.files, 1);
    assert_eq!(index.nodes[f.0.join("folder").to_str().unwrap()].size, 0);
}

#[test]
fn removal_transfers_size_to_a_surviving_hardlink() {
    let f = Fixture::new();
    let first = f.file("one/data");
    fs::create_dir(f.0.join("two")).unwrap();
    fs::hard_link(&first, f.0.join("two/data")).unwrap();
    let mut index = f.scan();
    let bytes = index.root.size;
    let apparent = index.root.apparent;
    let owner = index
        .nodes
        .values()
        .find(|n| !n.directory && n.size > 0)
        .unwrap()
        .path
        .clone();
    let parent = Path::new(owner.as_ref())
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    remove(&index, &json!({"paths":[parent]}), true).unwrap();
    prune_removed(Arc::make_mut(&mut index), &[parent]);
    assert_eq!(index.root.size, bytes);
    assert_eq!(index.root.apparent, apparent);
    assert_eq!(index.root.files, 1);
    let survivor = index.nodes.values().find(|n| !n.directory).unwrap();
    assert_eq!(survivor.size, bytes);
    validate_removal(&index, &[survivor.path.to_string()], true).unwrap();
}

#[test]
fn partial_removal_retains_failed_items_and_updates_successes() {
    let f = Fixture::new();
    let first = f.file("first").to_string_lossy().into_owned();
    let second = f.file("second").to_string_lossy().into_owned();
    let mut index = f.scan();
    let result = execute_removals(vec![first.clone(), second.clone()], |path| {
        if path == second {
            Err("simulated permission failure".into())
        } else {
            fs::remove_file(path).map_err(io)
        }
    })
    .unwrap();
    assert_eq!(result["completed"], json!([first]));
    assert!(result["error"].as_str().unwrap().contains("permission"));
    prune_removed(Arc::make_mut(&mut index), &[first.clone()]);
    assert!(!index.nodes.contains_key(first.as_str()));
    assert!(index.nodes.contains_key(second.as_str()));
    assert_eq!(index.root.files, 1);
    validate_removal(&index, &[second], true).unwrap();
}

#[test]
fn removal_does_not_accept_new_unscanned_siblings() {
    let f = Fixture::new();
    let victim = f.file("folder/victim");
    let mut index = f.scan();
    remove(&index, &json!({"paths":[victim]}), true).unwrap();
    f.file("folder/new-unscanned");
    prune_removed(
        Arc::make_mut(&mut index),
        &[victim.to_string_lossy().into_owned()],
    );
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
fn empty_trash_reconciles_only_cached_items_without_discovering_files() {
    let f = Fixture::new();
    f.file("trash/removed/nested");
    f.file("trash/survivor");
    let mut index = f.scan();
    fs::remove_dir_all(f.0.join("trash/removed")).unwrap();
    f.file("trash/new-unscanned");
    let missing = missing_cached_items(&index, &[f.0.join("trash").to_string_lossy().into_owned()]);
    assert_eq!(
        missing,
        vec![f.0.join("trash/removed").to_string_lossy().into_owned()]
    );
    prune_removed(Arc::make_mut(&mut index), &missing);
    assert_eq!(index.root.files, 1);
    assert!(
        !index
            .nodes
            .contains_key(f.0.join("trash/new-unscanned").to_str().unwrap())
    );
    assert!(
        validate_removal(
            &index,
            &[f.0.join("trash").to_string_lossy().into_owned()],
            true
        )
        .is_err()
    );
}

// Runs the real index builder with deterministic long paths, without filesystem
// cache or GUI variability. Isolate this test so unrelated test heaps are absent.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "release memory regression benchmark; run alone with --exact"]
fn long_path_index_memory_budget() {
    #[repr(C)]
    #[derive(Default)]
    struct Statistics {
        blocks: libc::c_uint,
        in_use: usize,
        maximum: usize,
        allocated: usize,
    }
    unsafe extern "C" {
        fn malloc_default_zone() -> *mut std::ffi::c_void;
        fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut Statistics);
    }
    fn allocated() -> Statistics {
        let mut stats = Statistics::default();
        unsafe { malloc_zone_statistics(malloc_default_zone(), &mut stats) };
        stats
    }
    let before = allocated();
    let root = format!("/fixture/{}", "long-directory-name/".repeat(8));
    let root = PathBuf::from(root.trim_end_matches('/'));
    let metadata = |directory| ScanMetadata {
        record: Record {
            device: 1,
            inode: 1,
            modified: 0,
            nanos: 0,
            size: 1,
        },
        directory,
        file: !directory,
        symlink: false,
        allocated: 4096,
        links: 1,
        evicted: false,
    };
    let mut walker = Walker {
        device: 1,
        seen: HashSet::default(),
        nodes: HashMap::default(),
        errors: vec![],
        error_count: 0,
        progress: None,
        last_progress: Instant::now(),
        directories: 0,
        files: 0,
        bytes: 0,
        current_directory: String::new(),
    };
    let mut raw = HashMap::default();
    walker.collect(root.clone(), metadata(true), &mut raw);
    for directory in 0..512 {
        let folder = root.join(format!("directory-{directory:04}"));
        walker.collect(folder.clone(), metadata(true), &mut raw);
        for file in 0..256 {
            walker.collect(
                folder.join(format!("file-{file:04}.bin")),
                metadata(false),
                &mut raw,
            );
        }
    }
    let node = walker.assemble(root.to_str().unwrap(), &mut raw);
    drop(raw);
    assert_eq!(node.files, 131_072);
    assert_eq!(walker.nodes.len(), 131_585);
    let after = allocated();
    let bytes = after.in_use.saturating_sub(before.in_use);
    eprintln!(
        "long-path index: {} entries, {:.2} MiB retained heap, {} allocations",
        walker.nodes.len(),
        bytes as f64 / 1048576.,
        after.blocks.saturating_sub(before.blocks)
    );
    assert!(
        bytes < 80 * 1024 * 1024,
        "long-path index exceeded 80 MiB: {bytes} bytes"
    );
    std::hint::black_box(walker);
    std::hint::black_box(node);
}
