use super::*;
use std::io::Write;
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Default)]
struct Statistics {
    blocks: libc::c_uint,
    in_use: usize,
    maximum: usize,
    allocated: usize,
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn malloc_default_zone() -> *mut std::ffi::c_void;
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut Statistics);
}
#[cfg(target_os = "macos")]
fn allocated() -> Statistics {
    let mut stats = Statistics::default();
    unsafe { malloc_zone_statistics(malloc_default_zone(), &mut stats) };
    stats
}

fn indexed(index: &Index, path: &Path) -> Node {
    index
        .node(index.lookup(path.to_str().unwrap()).unwrap().unwrap())
        .unwrap()
}
fn records(index: &Index) -> HashMap<String, Record> {
    index
        .all_ids()
        .unwrap()
        .into_iter()
        .map(|id| (index.path(id).unwrap(), index.node(id).unwrap().record))
        .collect()
}
fn synthetic_metadata(directory: bool) -> ScanMetadata {
    ScanMetadata {
        record: Record {
            device: 1,
            inode: 1,
            modified: 1,
            nanos: 0,
            size: 4000,
        },
        directory,
        file: !directory,
        symlink: false,
        allocated: 4096,
        links: 1,
        evicted: false,
    }
}

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
    assert_eq!(index.root().unwrap().apparent, 8192);
    assert_eq!(index.root().unwrap().files, 2);
    assert_eq!(
        index.root().unwrap().size,
        fs::metadata(file).unwrap().blocks() * 512
    );
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
        index.root().unwrap().size
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
        index.root().unwrap().size
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
    prune_removed(Arc::make_mut(&mut index), &completed).unwrap();
    let v = view(&index, &json!({"path":f.0.join("folder"),"fallback":true})).unwrap();
    assert_eq!(v["node"]["path"], f.0.to_str().unwrap());
    assert_eq!(v["count"], 0);
    assert_eq!(index.root().unwrap().files, 0);
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
    let index = f.scan();
    let id = index.lookup(file.to_str().unwrap()).unwrap().unwrap();
    let mut record = index.node(id).unwrap().record;
    record.device += 1;
    index.set_record(id, &record).unwrap();
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
    let mut walker = Walker::new(1, "/fixture", None).unwrap();
    let mut directories = HashMap::default();
    walker
        .collect(
            PathBuf::from("/fixture"),
            synthetic_metadata(true),
            &mut directories,
        )
        .unwrap();
    walker
        .collect(
            PathBuf::from("/fixture/archive"),
            synthetic_metadata(true),
            &mut directories,
        )
        .unwrap();
    for i in 0..count {
        walker
            .collect(
                PathBuf::from(format!("/fixture/archive/file-{i:06}")),
                synthetic_metadata(false),
                &mut directories,
            )
            .unwrap();
    }
    walker.index.finish().unwrap();
    let index = walker.index;
    let folder_path = "/fixture/archive";
    let folder = index.lookup(folder_path).unwrap().unwrap();
    for (offset, metric) in [(0, "size"), (80, "files"), (160, "apparent")] {
        let result = view(
            &index,
            &json!({"path":folder_path,"offset":offset,"metric":metric,"levels":3}),
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
            index.node(folder).unwrap().size
        );
    }
    assert_eq!(
        view(&index, &json!({"path":folder_path,"query":"file-649999"})).unwrap()["count"],
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
    assert_eq!(value["percent"], 100);
    assert_eq!(value["directories"], 3);
    assert_eq!(value["files"], 2);
    assert_eq!(value["bytes"], index.root().unwrap().size);
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
                index.live,
                index.root().unwrap().files,
                index.root().unwrap().size
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
        assert_eq!(parallel.root().unwrap().size, serial.root().unwrap().size);
        assert_eq!(
            parallel.root().unwrap().apparent,
            serial.root().unwrap().apparent
        );
        assert_eq!(parallel.root().unwrap().files, serial.root().unwrap().files);
        assert_eq!(records(&parallel), records(&serial));
        assert_eq!(parallel.live, serial.live);
        assert_eq!(response["errorCount"], serial_response["errorCount"]);
        assert_eq!(
            response["errorCount"],
            usize::from(cfg!(target_os = "linux"))
        );
        assert_eq!(
            progress.lock().unwrap()["files"],
            parallel.root().unwrap().files
        );
        let cycle = parallel
            .lookup(fixture.0.join("cycle").to_str().unwrap())
            .unwrap()
            .unwrap();
        assert!(
            parallel
                .children(cycle)
                .unwrap()
                .into_iter()
                .next()
                .is_none()
        );
        for id in parallel.all_ids().unwrap() {
            let node = parallel.node(id).unwrap();
            if !node.directory {
                continue;
            }
            assert_eq!(
                node.size,
                parallel
                    .children(id)
                    .unwrap()
                    .into_iter()
                    .map(|child| parallel.node(child).unwrap().size)
                    .sum::<u64>()
            );
            let children = parallel.children(id).unwrap();
            assert!(
                children
                    .windows(2)
                    .all(|pair| parallel.node(pair[0]).unwrap().name
                        < parallel.node(pair[1]).unwrap().name)
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
    assert_eq!(records(&standard), records(&bulk));
    assert_eq!(indexed(&bulk, &precise).record.nanos, 123_456_789);
    assert_eq!(standard.root().unwrap().size, bulk.root().unwrap().size);
    assert_eq!(
        standard.root().unwrap().apparent,
        bulk.root().unwrap().apparent
    );
    assert_eq!(standard.root().unwrap().files, bulk.root().unwrap().files);
    assert_eq!(standard.live, bulk.live);
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
    let untouched_id = index
        .lookup(f.0.join("untouched").to_str().unwrap())
        .unwrap()
        .unwrap();
    let untouched = index.node(untouched_id).unwrap().clone();
    let expected = index.root().unwrap().size - indexed(&index, &victim).size;
    remove(&index, &json!({"paths":[victim]}), true).unwrap();
    prune_removed(
        Arc::make_mut(&mut index),
        &[victim.to_string_lossy().into_owned()],
    )
    .unwrap();
    assert_eq!(index.id, original_id);
    assert_eq!(index.root().unwrap().size, expected);
    assert_eq!(index.root().unwrap().files, 2);
    assert!(index.lookup(victim.to_str().unwrap()).unwrap().is_none());
    assert_eq!(
        index
            .lookup(f.0.join("untouched").to_str().unwrap())
            .unwrap(),
        Some(untouched_id)
    );
    assert_eq!(untouched, index.node(untouched_id).unwrap());
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
    )
    .unwrap();
    assert_eq!(index.root().unwrap().files, 1);
    assert_eq!(indexed(&index, &f.0.join("folder")).size, 0);
}

#[test]
fn removal_transfers_size_to_a_surviving_hardlink() {
    let f = Fixture::new();
    let first = f.file("one/data");
    fs::create_dir(f.0.join("two")).unwrap();
    fs::hard_link(&first, f.0.join("two/data")).unwrap();
    let mut index = f.scan();
    let bytes = index.root().unwrap().size;
    let apparent = index.root().unwrap().apparent;
    let owner = index
        .all_ids()
        .unwrap()
        .into_iter()
        .find(|id| {
            let node = index.node(*id).unwrap();
            !node.directory && node.size > 0
        })
        .unwrap();
    let owner = index.path(owner).unwrap();
    let parent = Path::new(&owner)
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    remove(&index, &json!({"paths":[parent]}), true).unwrap();
    prune_removed(Arc::make_mut(&mut index), &[parent]).unwrap();
    assert_eq!(index.root().unwrap().size, bytes);
    assert_eq!(index.root().unwrap().apparent, apparent);
    assert_eq!(index.root().unwrap().files, 1);
    let survivor_id = index
        .all_ids()
        .unwrap()
        .into_iter()
        .find(|id| !index.node(*id).unwrap().directory)
        .unwrap();
    assert_eq!(index.node(survivor_id).unwrap().size, bytes);
    validate_removal(&index, &[index.path(survivor_id).unwrap()], true).unwrap();
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
    prune_removed(Arc::make_mut(&mut index), std::slice::from_ref(&first)).unwrap();
    assert!(index.lookup(&first).unwrap().is_none());
    assert!(index.lookup(&second).unwrap().is_some());
    assert_eq!(index.root().unwrap().files, 1);
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
    )
    .unwrap();
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
    let missing =
        missing_cached_items(&index, &[f.0.join("trash").to_string_lossy().into_owned()]).unwrap();
    assert_eq!(
        missing,
        vec![f.0.join("trash/removed").to_string_lossy().into_owned()]
    );
    prune_removed(Arc::make_mut(&mut index), &missing).unwrap();
    assert_eq!(index.root().unwrap().files, 1);
    assert!(
        index
            .lookup(f.0.join("trash/new-unscanned").to_str().unwrap())
            .unwrap()
            .is_none()
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
    let before = allocated();
    let root = format!("/fixture/{}", "long-directory-name/".repeat(8));
    let root = PathBuf::from(root.trim_end_matches('/'));
    let mut walker = Walker::new(1, root.to_str().unwrap(), None).unwrap();
    let mut raw = HashMap::default();
    walker
        .collect(root.clone(), synthetic_metadata(true), &mut raw)
        .unwrap();
    for directory in 0..512 {
        let folder = root.join(format!("directory-{directory:04}"));
        walker
            .collect(folder.clone(), synthetic_metadata(true), &mut raw)
            .unwrap();
        for file in 0..256 {
            walker
                .collect(
                    folder.join(format!("file-{file:04}.bin")),
                    synthetic_metadata(false),
                    &mut raw,
                )
                .unwrap();
        }
    }
    drop(raw);
    walker.index.finish().unwrap();
    assert_eq!(walker.index.root().unwrap().files, 131_072);
    assert_eq!(walker.index.live, 131_585);
    let after = allocated();
    let bytes = after.in_use.saturating_sub(before.in_use);
    eprintln!(
        "long-path index: {} entries, {:.2} MiB retained heap, {} allocations",
        walker.index.live,
        bytes as f64 / 1048576.,
        after.blocks.saturating_sub(before.blocks)
    );
    assert!(
        bytes < 24 * 1024 * 1024,
        "long-path index exceeded 24 MiB: {bytes} bytes"
    );
    std::hint::black_box(walker);
}

#[test]
fn scan_estimate_stays_monotonic_and_reserves_completion() {
    assert_eq!(scan_percent(0, 0, 1), 0);
    assert_eq!(scan_percent(0, 1, 3), 25);
    assert_eq!(scan_percent(25, 1, 20), 25);
    assert_eq!(scan_percent(25, 4, 0), 99);
}

#[test]
fn compact_paths_preserve_exact_names_and_scan_boundaries() {
    let mut walker = Walker::new(1, "/", None).unwrap();
    let mut directories = HashMap::default();
    for path in [
        "/",
        "/资料",
        "/资料/with spaces",
        "/资料/with spaces/é.bin",
        "/资料/other",
    ] {
        walker
            .collect(
                PathBuf::from(path),
                synthetic_metadata(!path.ends_with(".bin")),
                &mut directories,
            )
            .unwrap();
    }
    walker.index.finish().unwrap();
    let index = walker.index;
    for path in [
        "/",
        "/资料",
        "/资料/with spaces",
        "/资料/with spaces/é.bin",
        "/资料/other",
    ] {
        let id = index.lookup(path).unwrap().unwrap();
        assert_eq!(index.path(id).unwrap(), path);
        assert_eq!(index.shallow(id).unwrap()["path"], path);
    }
    assert!(
        index
            .lookup("/资料/with spaces/../other")
            .unwrap()
            .is_none()
    );
    assert!(index.lookup("/资料/with spaces/missing").unwrap().is_none());
    assert!(
        index
            .lookup("/资料/with spaces/é.bin/child")
            .unwrap()
            .is_none()
    );
    let mut subtree = Walker::new(1, "/fixture/subtree", None).unwrap();
    subtree
        .collect(
            PathBuf::from("/fixture/subtree"),
            synthetic_metadata(true),
            &mut HashMap::default(),
        )
        .unwrap();
    subtree.index.finish().unwrap();
    assert!(
        subtree
            .index
            .lookup("/fixture/subtree-other")
            .unwrap()
            .is_none()
    );
    assert!(
        subtree
            .index
            .lookup("/fixture/subtree/../outside")
            .unwrap()
            .is_none()
    );
}

#[test]
fn large_removal_reclaims_storage_and_preserves_surviving_paths() {
    let f = Fixture::new();
    for i in 0..1200 {
        f.file(&format!("victim/file-{i:04}"));
    }
    let keep = f.file("保留/nested/keep.bin");
    let mut index = f.scan();
    let original_pages: i64 = index
        .db()
        .unwrap()
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    let expected_bytes = indexed(&index, &keep).size;
    let response = remove(&index, &json!({"paths":[f.0.join("victim")]}), true).unwrap();
    assert_eq!(response["completed"], json!([f.0.join("victim")]));
    prune_removed(
        Arc::make_mut(&mut index),
        &[f.0.join("victim").to_string_lossy().into_owned()],
    )
    .unwrap();
    assert_eq!(index.live, 4);
    assert_eq!(index.all_ids().unwrap().len(), 4);
    assert_eq!(index.children(0).unwrap().len(), 1);
    let pages: i64 = index
        .db()
        .unwrap()
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    assert!(pages < original_pages);
    assert_eq!(index.root().unwrap().files, 1);
    assert_eq!(index.root().unwrap().size, expected_bytes);
    assert_eq!(
        index
            .path(index.lookup(keep.to_str().unwrap()).unwrap().unwrap())
            .unwrap(),
        keep.to_str().unwrap()
    );
    assert!(
        index
            .lookup(f.0.join("victim/file-0000").to_str().unwrap())
            .unwrap()
            .is_none()
    );
    let response = query(&index, &f.0.join("保留/nested"));
    assert_eq!(response["count"], 1);
    assert_eq!(response["ancestors"].as_array().unwrap().len(), 3);
    validate_removal(
        &index,
        &[f.0.join("保留").to_string_lossy().into_owned()],
        true,
    )
    .unwrap();
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "real-tree memory benchmark; set DISKTREE_BENCH_ROOT and run alone"]
fn scan_memory_benchmark() {
    let Some(root) = std::env::var_os("DISKTREE_BENCH_ROOT") else {
        eprintln!("Set DISKTREE_BENCH_ROOT to measure an existing directory.");
        return;
    };
    let root = PathBuf::from(root);
    let before = allocated();
    let started = Instant::now();
    let (index, _) = scan_with_workers(&root, None, 4).unwrap();
    let after = allocated();
    let pages: i64 = index
        .db()
        .unwrap()
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    eprintln!(
        "scan memory: {:?}, {} entries, {} files, {:.2} MiB retained heap, {} allocations, {:.2} MiB disk index",
        started.elapsed(),
        index.live,
        index.root().unwrap().files,
        after.in_use.saturating_sub(before.in_use) as f64 / 1048576.,
        after.blocks.saturating_sub(before.blocks),
        pages as f64 * 4096. / 1048576.
    );
    std::hint::black_box(&index);
    drop(index);
    eprintln!(
        "after drop: {:.2} MiB retained heap",
        allocated().in_use.saturating_sub(before.in_use) as f64 / 1048576.
    );
}

#[cfg(target_os = "macos")]
#[test]
fn volume_trash_root_remains_discoverable_when_it_is_the_scan_root() {
    let uid = unsafe { libc::getuid() };
    let root = format!("/fixture/.Trashes/{uid}");
    let mut walker = Walker::new(1, &root, None).unwrap();
    walker
        .collect(
            PathBuf::from(&root),
            synthetic_metadata(true),
            &mut HashMap::default(),
        )
        .unwrap();
    walker.index.finish().unwrap();
    assert!(cached_trash_roots(&walker.index).unwrap().contains(&root));
}

#[test]
fn unicode_and_literal_filters_preserve_name_matching() {
    let f = Fixture::new();
    for i in 0..100 {
        f.file(&format!("entry-{i:03}"));
    }
    f.file("ÉCOLE_%.bin");
    let index = f.scan();
    for query in ["école", "%", "_"] {
        let result = view(&index, &json!({"path":f.0,"query":query})).unwrap();
        assert_eq!(result["count"], 1);
        assert_eq!(result["children"][0]["name"], "ÉCOLE_%.bin");
    }
}

#[test]
fn failed_index_write_stops_directory_readers() {
    let f = Fixture::new();
    for i in 0..600 {
        f.file(&format!("entry-{i}"));
    }
    let meta = fs::symlink_metadata(&f.0).unwrap();
    let mut walker = Walker::new(meta.dev(), f.0.to_str().unwrap(), None).unwrap();
    walker.add(&f.0, meta.into(), NO_PARENT).unwrap();
    walker
        .index
        .db()
        .unwrap()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert!(read_directories(&f.0, 4, true, &mut walker).is_err());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "3.3 million-file memory gate; run alone in release mode with --exact"]
fn million_entry_memory_budget() {
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    let before = allocated();
    let running = AtomicBool::new(true);
    let peak_heap = AtomicUsize::new(before.in_use);
    let (walker, result) = std::thread::scope(|scope| {
        scope.spawn(|| {
            while running.load(Ordering::Relaxed) {
                peak_heap.fetch_max(allocated().in_use, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let result = (|| -> Result<_> {
            let root = PathBuf::from(
                format!("/fixture/{}", "long-directory-name/".repeat(8)).trim_end_matches('/'),
            );
            let mut walker = Walker::new(1, root.to_str().unwrap(), None)?;
            walker.add(&root, synthetic_metadata(true), NO_PARENT)?;
            for folder in 0..660_000 {
                let path = root.join(format!("directory-{folder:06}"));
                let parent = walker.add(&path, synthetic_metadata(true), 0)?;
                for file in 0..5 {
                    walker.add(
                        &path.join(format!("file-{file}.bin")),
                        synthetic_metadata(false),
                        parent,
                    )?;
                }
            }
            walker.index.finish()?;
            let result = view(&walker.index, &json!({"path":root,"levels":3}))?;
            Ok((walker, result))
        })();
        running.store(false, Ordering::Relaxed);
        result
    })
    .unwrap();
    assert_eq!(walker.index.live, 3_960_001);
    assert_eq!(walker.index.root().unwrap().files, 3_300_000);
    assert_eq!(result["count"], 660_000);
    assert_eq!(result["children"].as_array().unwrap().len(), PAGE_SIZE);
    let after = allocated();
    let retained = after.in_use.saturating_sub(before.in_use);
    let peak = peak_heap
        .load(Ordering::Relaxed)
        .max(after.in_use)
        .saturating_sub(before.in_use);
    eprintln!(
        "3.3 million files: {:.2} MiB retained heap, {:.2} MiB peak heap",
        retained as f64 / 1048576.,
        peak as f64 / 1048576.
    );
    assert!(
        retained < 24 * 1024 * 1024,
        "retained heap exceeded 24 MiB: {retained}"
    );
    assert!(peak < 48 * 1024 * 1024, "peak heap exceeded 48 MiB: {peak}");
    std::hint::black_box(walker);
}

#[test]
fn permanent_removal_reports_progress_without_following_links() {
    let f = Fixture::new();
    for name in [
        "tree/a",
        "tree/b",
        "tree/deep/c",
        "tree/deep/er/d",
        "outside/keep",
    ] {
        f.file(name);
    }
    std::os::unix::fs::symlink(f.0.join("outside"), f.0.join("tree/deep/link")).unwrap();
    let index = f.scan();
    let tree = f.0.join("tree").to_string_lossy().into_owned();
    let response = dispatch("/delete", &json!({"id":"missing","paths":[tree]}));
    assert!(response.is_err(), "removal requires the active scan");
    let published = Arc::new(Mutex::new(Value::Null));
    let result = remove_with_progress(
        &index,
        &json!({"paths":[tree]}),
        true,
        Some(published.clone()),
    )
    .unwrap();
    assert_eq!(result["completed"], json!([tree]));
    assert!(!f.0.join("tree").exists());
    assert!(f.0.join("outside/keep").exists());
    let progress = published.lock().unwrap().clone();
    assert_eq!(progress["percent"], 100);
    assert_eq!(progress["items"], 1);
    assert_eq!(progress["files"], progress["totalFiles"]);
    assert_eq!(progress["bytes"], progress["totalBytes"]);
}

#[test]
fn removing_a_tree_refuses_a_directory_replaced_since_it_was_checked() {
    let f = Fixture::new();
    let victim = f.file("victim/file");
    let meta = fs::metadata(f.0.join("victim")).unwrap();
    let mut files = 0;
    let error = remove_tree(
        &f.0.join("victim").to_string_lossy(),
        meta.dev() + 1,
        &mut |_| files += 1,
    )
    .unwrap_err();
    assert!(error.contains("mounted volumes"), "{error}");
    assert!(victim.exists());
    assert_eq!(files, 0);
}
