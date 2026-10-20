use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    ffi::CString,
    fs::{self, Metadata},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
mod macos;

type Result<T> = std::result::Result<T, String>;
const PAGE_SIZE: usize = 80;
const MAP_BUDGET: usize = 240;
static INDEX: Mutex<Option<Arc<Index>>> = Mutex::new(None);
static PROGRESS: Mutex<Option<(String, Arc<Mutex<Value>>)>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq)]
struct Record {
    device: u64,
    inode: u64,
    modified: i64,
    nanos: i64,
    size: u64,
}
impl From<&Metadata> for Record {
    fn from(m: &Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
            modified: m.mtime(),
            nanos: m.mtime_nsec(),
            size: m.len(),
        }
    }
}
// The native bulk reader and the portable stat reader retain identical fields
// for both measurement and the deletion-time identity checks.
struct ScanMetadata {
    record: Record,
    directory: bool,
    file: bool,
    symlink: bool,
    allocated: u64,
    links: u64,
    evicted: bool,
}
impl From<Metadata> for ScanMetadata {
    fn from(meta: Metadata) -> Self {
        Self {
            record: Record::from(&meta),
            directory: meta.is_dir(),
            file: meta.is_file(),
            symlink: meta.file_type().is_symlink(),
            allocated: meta.blocks() * 512,
            links: meta.nlink(),
            evicted: cloud_only(&meta),
        }
    }
}
#[cfg(target_os = "macos")]
impl From<macos::Entry> for ScanMetadata {
    fn from(entry: macos::Entry) -> Self {
        let (device, inode) = entry.identity();
        Self {
            record: Record {
                device,
                inode,
                modified: entry.modified(),
                nanos: entry.nanos(),
                size: entry.apparent(),
            },
            directory: entry.kind() == macos::Kind::Directory,
            file: entry.kind() == macos::Kind::File,
            symlink: entry.kind() == macos::Kind::Link,
            allocated: entry.allocated(),
            links: entry.links(),
            evicted: entry.evicted(),
        }
    }
}

// Unsupported filesystems use the portable reader before any entry has been
// emitted, so fallback cannot duplicate records or hard-link accounting.
fn directory_entries(
    path: &Path,
    bulk: bool,
) -> Result<Box<dyn Iterator<Item = Result<(PathBuf, ScanMetadata)>> + '_>> {
    #[cfg(target_os = "macos")]
    if bulk {
        let mut reader = macos::read_dir(path).map_err(io)?;
        let first = reader.next();
        let unsupported = matches!(&first, Some(Err(error)) if matches!(error.raw_os_error(), Some(libc::ENOTSUP | libc::ENOSYS | libc::EINVAL)));
        if !unsupported {
            return Ok(Box::new(first.into_iter().chain(reader).map(
                move |entry| {
                    let entry = entry.map_err(io)?;
                    Ok((entry.path(path), entry.into()))
                },
            )));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = bulk;
    Ok(Box::new(fs::read_dir(path).map_err(io)?.map(|entry| {
        let path = entry.map_err(io)?.path();
        let meta = fs::symlink_metadata(&path).map_err(io)?;
        Ok((path, meta.into()))
    })))
}

#[derive(Clone, Debug)]
struct Node {
    path: String,
    name: String,
    directory: bool,
    size: u64,
    apparent: u64,
    files: u64,
    modified: f64,
    children: Vec<Arc<Node>>,
}
impl Node {
    fn shallow(&self) -> Value {
        json!({"path":self.path,"name":self.name,"directory":self.directory,"size":self.size,
            "apparent":self.apparent,"files":self.files,"modified":self.modified,"children":[]})
    }
    fn weight(&self, metric: &str) -> u64 {
        match metric {
            "apparent" => self.apparent,
            "files" => self.files,
            _ => self.size,
        }
    }
}
#[derive(Clone)]
struct Index {
    id: String,
    root: Arc<Node>,
    nodes: HashMap<String, Arc<Node>>,
    records: HashMap<String, Record>,
    shared: HashSet<(u64, u64)>,
}
struct Walker {
    device: u64,
    seen: HashSet<(u64, u64)>,
    nodes: HashMap<String, Arc<Node>>,
    records: HashMap<String, Record>,
    errors: Vec<String>,
    error_count: usize,
    progress: Option<Arc<Mutex<Value>>>,
    last_progress: Instant,
    directories: u64,
    files: u64,
    bytes: u64,
    current_directory: String,
}
fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
fn state_file() -> PathBuf {
    if let Some(path) = std::env::var_os("DISKTREE_STATE") {
        return path.into();
    }
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Disktree/state.json")
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/state"))
            .join("nsh-disktree/state.json")
    }
}
fn saved_location(file: &Path) -> PathBuf {
    fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| v["directory"].as_str().map(PathBuf::from))
        .filter(|p| p.is_dir())
        .unwrap_or_else(home)
}
fn save_location(path: &str, file: &Path) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    // Distinct names allow a development and packaged host to save concurrently.
    let temporary = file.with_extension(format!(
        "{}.{}.new",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, json!({"directory":path}).to_string()).map_err(io)?;
    fs::rename(&temporary, file).map_err(io)
}
fn absolute(path: &str) -> Result<PathBuf> {
    let expanded = if path == "~" {
        home()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(path)
    };
    let full = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir().map_err(io)?.join(expanded)
    };
    let mut clean = PathBuf::new();
    for part in full.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    Ok(clean)
}
// statvfs block counts are u32 on macOS and u64 on Linux.
fn block_bytes(count: impl Into<u64>, block: u64) -> u64 {
    count.into().saturating_mul(block)
}
fn disk_usage(path: &Path) -> Result<(u64, u64)> {
    let name = CString::new(path.as_os_str().as_bytes()).map_err(io)?;
    let mut data = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // statvfs initializes the output on success; the CString lives through the call.
    if unsafe { libc::statvfs(name.as_ptr(), data.as_mut_ptr()) } != 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    let data = unsafe { data.assume_init() };
    let block = data.f_frsize;
    Ok((
        block_bytes(data.f_blocks, block),
        block_bytes(data.f_bavail, block),
    ))
}
fn cloud_only(_m: &Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        _m.st_flags() & 0x40000000 != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
impl Walker {
    fn error(&mut self, message: String) {
        self.error_count += 1;
        if self.errors.len() < 100 {
            self.errors.push(message);
        }
    }
    fn publish_progress(&mut self, force: bool) {
        if !force && self.last_progress.elapsed() < Duration::from_millis(100) {
            return;
        }
        if let Some(progress) = &self.progress {
            if let Ok(mut value) = progress.lock() {
                *value = json!({"path":self.current_directory,"directories":self.directories,"files":self.files,"bytes":self.bytes,"errors":self.error_count});
            }
        }
        self.last_progress = Instant::now();
    }
    fn collect(
        &mut self,
        path: PathBuf,
        meta: ScanMetadata,
        raw: &mut HashMap<String, PendingNode>,
    ) {
        let text = path.to_string_lossy().into_owned();
        self.records.insert(text.clone(), meta.record.clone());
        let mut node = Node {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".into()),
            path: text.clone(),
            directory: meta.directory,
            size: 0,
            apparent: 0,
            files: 0,
            modified: meta.record.modified as f64 + meta.record.nanos as f64 / 1e9,
            children: vec![],
        };
        if meta.record.device == self.device && !meta.symlink {
            if meta.file {
                self.files += 1;
                let parent = path.parent().unwrap_or(&path).to_string_lossy();
                if self.current_directory != parent {
                    self.current_directory = parent.into_owned();
                }
                node.files = 1;
                if meta.links <= 1 || self.seen.insert((meta.record.device, meta.record.inode)) {
                    node.size = meta.allocated;
                    node.apparent = meta.record.size;
                    self.bytes += node.size;
                }
            } else if meta.directory {
                self.directories += 1;
                self.current_directory = text.clone();
                if meta.evicted {
                    self.error(format!("{text}: cloud-only (skipped)"));
                }
            }
        }
        let parent = path
            .parent()
            .and_then(|p| p.to_str())
            .and_then(|p| raw.get_mut(p));
        if node.directory {
            if let Some(parent) = parent {
                parent.children.push(text.clone());
            }
            raw.insert(
                text,
                PendingNode {
                    node,
                    children: vec![],
                },
            );
        } else {
            // Leaves are already complete. Attach them now instead of keeping
            // another path-keyed node and looking it up again during assembly.
            let node = Arc::new(node);
            if let Some(parent) = parent {
                parent.node.size += node.size;
                parent.node.apparent += node.apparent;
                parent.node.files += node.files;
                parent.node.children.push(node.clone());
            }
            self.nodes.insert(text, node);
        }
        self.publish_progress(false);
    }
    fn assemble(&mut self, path: &str, raw: &mut HashMap<String, PendingNode>) -> Arc<Node> {
        let PendingNode { mut node, children } = raw.remove(path).expect("enumerated node");
        for child in children {
            let child = self.assemble(&child, raw);
            node.size += child.size;
            node.apparent += child.apparent;
            node.files += child.files;
            node.children.push(child);
        }
        node.children
            .sort_by(|a, b| b.size.cmp(&a.size).then(a.name.cmp(&b.name)));
        let node = Arc::new(node);
        self.nodes.insert(node.path.clone(), node.clone());
        node
    }
}
struct PendingNode {
    node: Node,
    children: Vec<String>,
}
struct DirectoryQueue {
    paths: VecDeque<PathBuf>,
    active: usize,
}
type Entries = Vec<(PathBuf, Result<ScanMetadata>)>;

// A fixed number of directory readers feeds bounded batches to one reducer.
// Only the reducer owns the index, hard-link set, and progress counters.
fn read_directories(
    path: &Path,
    workers: usize,
    bulk: bool,
    walker: &mut Walker,
    raw: &mut HashMap<String, PendingNode>,
) {
    let queue = (
        Mutex::new(DirectoryQueue {
            paths: VecDeque::from([path.to_owned()]),
            active: 0,
        }),
        Condvar::new(),
    );
    let (sender, receiver) = mpsc::sync_channel::<Entries>(workers * 2);
    let device = walker.device;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let directory = {
                        let mut state = queue.0.lock().unwrap();
                        loop {
                            if let Some(path) = state.paths.pop_front() {
                                state.active += 1;
                                break path;
                            }
                            if state.active == 0 {
                                return;
                            }
                            state = queue.1.wait(state).unwrap();
                        }
                    };
                    let mut batch = Vec::with_capacity(128);
                    let mut directories = Vec::new();
                    let flush = |batch: &mut Entries, directories: &mut Vec<PathBuf>| {
                        if batch.is_empty() {
                            return;
                        }
                        // Publish a directory's metadata before scheduling its children:
                        // the reducer can always attach entries to an existing parent.
                        if sender
                            .send(std::mem::replace(batch, Vec::with_capacity(128)))
                            .is_err()
                        {
                            return;
                        }
                        if !directories.is_empty() {
                            let mut state = queue.0.lock().unwrap();
                            state.paths.extend(directories.drain(..));
                            queue.1.notify_all();
                        }
                    };
                    match directory_entries(&directory, bulk) {
                        Err(error) => batch.push((directory.clone(), Err(error))),
                        Ok(entries) => {
                            for entry in entries {
                                let (path, metadata) = match entry {
                                    Err(error) => (directory.clone(), Err(error)),
                                    Ok((path, metadata)) => {
                                        let metadata = if path.to_str().is_none() {
                                            Err("Path is not valid UTF-8 (skipped)".into())
                                        } else {
                                            Ok(metadata)
                                        };
                                        (path, metadata)
                                    }
                                };
                                if let Ok(meta) = &metadata {
                                    if meta.directory
                                        && meta.record.device == device
                                        && !meta.symlink
                                        && !meta.evicted
                                    {
                                        directories.push(path.clone());
                                    }
                                }
                                batch.push((path, metadata));
                                if batch.len() == 128 {
                                    flush(&mut batch, &mut directories);
                                }
                            }
                        }
                    }
                    flush(&mut batch, &mut directories);
                    let mut state = queue.0.lock().unwrap();
                    state.active -= 1;
                    queue.1.notify_all();
                }
            });
        }
        drop(sender);
        for batch in receiver {
            for (path, metadata) in batch {
                match metadata {
                    Ok(meta) => walker.collect(path, meta, raw),
                    Err(error) => walker.error(format!("{}: {error}", path.display())),
                }
            }
        }
    });
}

#[cfg(test)]
fn scan(path: &Path) -> Result<(Arc<Index>, Value)> {
    scan_with_progress(path, None)
}
fn scan_with_progress(
    path: &Path,
    progress: Option<Arc<Mutex<Value>>>,
) -> Result<(Arc<Index>, Value)> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .unwrap_or(2);
    scan_with_workers(path, progress, workers)
}
fn scan_with_workers(
    path: &Path,
    progress: Option<Arc<Mutex<Value>>>,
    workers: usize,
) -> Result<(Arc<Index>, Value)> {
    scan_with_reader(path, progress, workers, true)
}
fn scan_with_reader(
    path: &Path,
    progress: Option<Arc<Mutex<Value>>>,
    workers: usize,
    bulk: bool,
) -> Result<(Arc<Index>, Value)> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Choose a directory, not a symbolic link".into());
    }
    let root_path = path.to_str().ok_or("Path is not valid UTF-8 (skipped)")?;
    let skip_root = cloud_only(&meta);
    let mut walker = Walker {
        device: meta.dev(),
        seen: HashSet::default(),
        nodes: HashMap::default(),
        records: HashMap::default(),
        errors: vec![],
        error_count: 0,
        progress,
        last_progress: Instant::now() - Duration::from_millis(100),
        directories: 0,
        files: 0,
        bytes: 0,
        current_directory: String::new(),
    };
    let mut raw = HashMap::default();
    walker.collect(path.to_owned(), meta.into(), &mut raw);
    if !skip_root {
        read_directories(path, workers.max(1), bulk, &mut walker, &mut raw);
    }
    let root = walker.assemble(root_path, &mut raw);
    walker.publish_progress(true);
    let (total, free) = disk_usage(path)?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    );
    let response = json!({"id":id,"root":root.shallow(),"total":total,"free":free,"errors":walker.errors,"errorCount":walker.error_count});
    let index = Arc::new(Index {
        id,
        root,
        nodes: walker.nodes,
        records: walker.records,
        shared: walker.seen,
    });
    Ok((index, response))
}
fn aggregate(items: &[Arc<Node>], path: String, name: String) -> Value {
    json!({"path":path,"name":name,"directory":false,"size":items.iter().map(|n|n.size).sum::<u64>(),
        "apparent":items.iter().map(|n|n.apparent).sum::<u64>(),"files":items.iter().map(|n|n.files).sum::<u64>(),
        "modified":0,"children":[],"aggregate":true})
}
fn bounded(
    items: &[Arc<Node>],
    depth: usize,
    levels: usize,
    budget: &mut usize,
    path: &str,
) -> Vec<Value> {
    let selected = items.len().min(budget.saturating_sub(1));
    *budget -= selected;
    let mut output: Vec<_> = items[..selected].iter().map(|n| n.shallow()).collect();
    if selected < items.len() {
        *budget -= 1;
        output.push(aggregate(
            &items[selected..],
            format!("{path}/#remaining-{depth}-{budget}"),
            format!("{} more items", items.len() - selected),
        ));
    }
    for (item, node) in output[..selected].iter_mut().zip(items) {
        if !node.children.is_empty() && depth < levels && *budget > 1 {
            item["children"] = json!(bounded(
                &node.children,
                depth + 1,
                levels,
                budget,
                &node.path
            ));
        }
    }
    output
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().ok_or_else(|| format!("Missing {key}"))
}
fn inside(path: &str, parent: &str) -> bool {
    path != parent && Path::new(path).starts_with(parent)
}
fn view(index: &Index, data: &Value) -> Result<Value> {
    let mut path = string(data, "path")?.to_owned();
    let metric = data["metric"].as_str().unwrap_or("size");
    if !["size", "apparent", "files"].contains(&metric) {
        return Err("Invalid metric".into());
    }
    if data["fallback"].as_bool().unwrap_or(false) {
        if path != index.root.path && !inside(&path, &index.root.path) {
            return Err("Directory is outside the scan".into());
        }
        while !index.nodes.contains_key(&path) && path != index.root.path {
            path = Path::new(&path)
                .parent()
                .ok_or("Directory is outside the scan")?
                .to_string_lossy()
                .into_owned();
        }
    }
    let node = index
        .nodes
        .get(&path)
        .ok_or("Directory no longer exists. Scan again.")?;
    if !node.directory {
        return Err("Choose a directory".into());
    }
    let query = data["query"].as_str().unwrap_or("").to_lowercase();
    let hidden = data["hidden"].as_bool().unwrap_or(true);
    let mut children: Vec<_> = node
        .children
        .iter()
        .filter(|n| (hidden || !n.name.starts_with('.')) && n.name.to_lowercase().contains(&query))
        .cloned()
        .collect();
    children.sort_by(|a, b| {
        b.weight(metric)
            .cmp(&a.weight(metric))
            .then(a.name.cmp(&b.name))
    });
    let offset = (data["offset"].as_u64().unwrap_or(0) as usize)
        .min(children.len().saturating_sub(1) / PAGE_SIZE * PAGE_SIZE);
    let end = (offset + PAGE_SIZE).min(children.len());
    let page = &children[offset..end];
    let levels = data["levels"].as_u64().unwrap_or(2).clamp(1, 3) as usize;
    let mut budget = MAP_BUDGET;
    let mut tiles = bounded(page, 1, levels, &mut budget, &path);
    let rest: Vec<_> = children[..offset]
        .iter()
        .chain(children[end..].iter())
        .cloned()
        .collect();
    if !rest.is_empty() {
        tiles.push(aggregate(
            &rest,
            format!("{path}/#other-pages"),
            format!("{} items on other pages", rest.len()),
        ));
    }
    let mut ancestors = vec![];
    let mut cursor = Path::new(&path);
    loop {
        ancestors.push(
            index
                .nodes
                .get(cursor.to_str().ok_or("Invalid path")?)
                .ok_or("Missing ancestor")?
                .shallow(),
        );
        if cursor == Path::new(&index.root.path) {
            break;
        }
        cursor = cursor.parent().ok_or("Missing ancestor")?;
    }
    ancestors.reverse();
    Ok(
        json!({"node":node.shallow(),"children":page.iter().map(|n|n.shallow()).collect::<Vec<_>>(),"map":tiles,
        "ancestors":ancestors,"offset":offset,"count":children.len(),"pageSize":PAGE_SIZE}),
    )
}
fn validate_removal(index: &Index, paths: &[String], permanent: bool) -> Result<()> {
    if paths.is_empty() {
        return Err("Nothing marked".into());
    }
    for path in paths {
        if !inside(path, &index.root.path) || !index.records.contains_key(path) {
            return Err("Only scanned descendants can be removed".into());
        }
        if paths.iter().any(|other| inside(path, other)) {
            return Err("Overlapping selections".into());
        }
        let current = fs::symlink_metadata(path).map_err(io)?;
        if Record::from(&current) != index.records[path] {
            return Err("Selection changed since scan. Scan again before removing it.".into());
        }
        let mut parent = Path::new(path).parent();
        while let Some(p) = parent {
            let meta = fs::symlink_metadata(p).map_err(io)?;
            let old = index
                .records
                .get(p.to_str().ok_or("Invalid path")?)
                .ok_or("Directory changed. Scan again.")?;
            if meta.file_type().is_symlink() || meta.dev() != old.device || meta.ino() != old.inode
            {
                return Err("Directory changed. Scan again.".into());
            }
            if p == Path::new(&index.root.path) {
                break;
            }
            parent = p.parent();
        }
        for (descendant, old) in &index.records {
            if descendant == path || inside(descendant, path) {
                if permanent && old.device != index.records[&index.root.path].device {
                    return Err("Cannot permanently delete across mounted volumes".into());
                }
                let meta = fs::symlink_metadata(descendant).map_err(io)?;
                if Record::from(&meta) != *old {
                    return Err(
                        "Contents changed since scan. Scan again before removing them.".into(),
                    );
                }
            }
        }
    }
    Ok(())
}
fn trash_command(path: &str) -> Command {
    if cfg!(target_os = "macos") {
        let mut cmd = Command::new("osascript");
        cmd.args(["-e","on run argv\nset targetItem to POSIX file (item 1 of argv) as alias\ntell application \"Finder\" to delete targetItem\nend run",path]);
        cmd
    } else {
        let mut cmd = Command::new("gio");
        cmd.args(["trash", "--", path]);
        cmd
    }
}
fn empty_command() -> Command {
    if cfg!(target_os = "macos") {
        let mut cmd = Command::new("osascript");
        cmd.args(["-e", "tell application \"Finder\" to empty trash"]);
        cmd
    } else {
        let mut cmd = Command::new("gio");
        cmd.args(["trash", "--empty"]);
        cmd
    }
}
fn execute(cmd: &mut Command) -> Result<()> {
    let output = cmd.output().map_err(io)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}
fn remove(index: &Index, data: &Value, permanent: bool) -> Result<Value> {
    let mut unique = HashSet::default();
    let paths: Vec<String> = data["paths"]
        .as_array()
        .ok_or("Missing paths")?
        .iter()
        .map(|p| {
            p.as_str()
                .map(String::from)
                .ok_or_else(|| "Invalid path".into())
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|p| unique.insert(p.clone()))
        .collect();
    validate_removal(index, &paths, permanent)?;
    execute_removals(paths, |path| {
        if permanent {
            let meta = fs::symlink_metadata(path).map_err(io)?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                fs::remove_dir_all(path).map_err(io)
            } else {
                fs::remove_file(path).map_err(io)
            }
        } else {
            execute(&mut trash_command(path))
        }
    })
}
fn execute_removals(
    paths: Vec<String>,
    mut action: impl FnMut(&str) -> Result<()>,
) -> Result<Value> {
    let mut completed = vec![];
    for path in paths {
        if let Err(error) = action(&path) {
            return Ok(json!({"completed":completed,"error":error}));
        }
        completed.push(path);
    }
    Ok(json!({"completed":completed}))
}

fn mark_ancestors(index: &Index, path: &str, dirty: &mut HashSet<String>) {
    let mut cursor = Path::new(path).parent();
    while let Some(parent) = cursor.and_then(Path::to_str) {
        if !index.nodes.contains_key(parent) {
            break;
        }
        dirty.insert(parent.to_owned());
        if parent == index.root.path {
            break;
        }
        cursor = Path::new(parent).parent();
    }
}

// Edit only known removed subtrees and their ancestors. No filesystem walk or
// new scan ID is needed; unrelated branches retain their existing Arc<Node>.
fn prune_removed(index: &mut Index, completed: &[String]) {
    let mut removed = HashSet::default();
    let mut dirty = HashSet::default();
    let mut parents = HashSet::default();
    let mut lost_owners = HashMap::default();
    let mut pending: Vec<_> = completed
        .iter()
        .filter_map(|p| index.nodes.get(p).cloned())
        .collect();
    for path in completed {
        mark_ancestors(index, path, &mut dirty);
        if let Some(parent) = Path::new(path).parent().and_then(Path::to_str) {
            parents.insert(parent.to_owned());
        }
    }
    while let Some(node) = pending.pop() {
        if !removed.insert(node.path.clone()) {
            continue;
        }
        if let Some(record) = index.records.get(&node.path) {
            let identity = (record.device, record.inode);
            if !node.directory
                && index.shared.contains(&identity)
                && (node.size > 0 || node.apparent > 0)
            {
                lost_owners.insert(identity, (node.size, node.apparent));
            }
        }
        pending.extend(node.children.iter().cloned());
        index.nodes.remove(&node.path);
        index.records.remove(&node.path);
    }
    // A hard link can survive outside a removed subtree. Transfer accounting
    // from a removed owner to one surviving name, without statting that tree.
    let mut transfers = vec![];
    if !lost_owners.is_empty() {
        for (path, record) in &index.records {
            if let Some(weights) = lost_owners.remove(&(record.device, record.inode)) {
                transfers.push((path.clone(), weights));
                if lost_owners.is_empty() {
                    break;
                }
            }
        }
    }
    for (path, (size, apparent)) in transfers {
        if let Some(node) = index.nodes.get_mut(&path) {
            let node = Arc::make_mut(node);
            node.size = size;
            node.apparent = apparent;
        }
        mark_ancestors(index, &path, &mut dirty);
    }
    let mut dirty: Vec<_> = dirty.into_iter().collect();
    dirty.sort_by_key(|path| std::cmp::Reverse(path.len()));
    for path in dirty {
        let Some(old) = index.nodes.get(&path) else {
            continue;
        };
        let mut node = (**old).clone();
        node.children = old
            .children
            .iter()
            .filter_map(|child| index.nodes.get(&child.path).cloned())
            .collect();
        node.size = node.children.iter().map(|child| child.size).sum();
        node.apparent = node.children.iter().map(|child| child.apparent).sum();
        node.files = node.children.iter().map(|child| child.files).sum();
        node.children
            .sort_by(|a, b| b.size.cmp(&a.size).then(a.name.cmp(&b.name)));
        // Our removal changes the direct parent's mtime. Refresh its record
        // only if the surviving immediate names still match the cached tree;
        // an unrelated new entry must continue to invalidate later deletion.
        if parents.contains(&path) {
            if let (Some(old_record), Ok(meta)) =
                (index.records.get(&path), fs::symlink_metadata(&path))
            {
                let current = Record::from(&meta);
                let expected: HashSet<_> = node
                    .children
                    .iter()
                    .map(|child| child.path.as_str())
                    .collect();
                let matches = fs::read_dir(&path)
                    .ok()
                    .and_then(|entries| {
                        entries
                            .map(|entry| entry.map(|e| e.path()).map_err(io))
                            .collect::<Result<Vec<_>>>()
                            .ok()
                    })
                    .is_some_and(|entries| {
                        entries.len() == expected.len()
                            && entries
                                .iter()
                                .all(|p| p.to_str().is_some_and(|p| expected.contains(p)))
                    });
                if meta.is_dir()
                    && !meta.file_type().is_symlink()
                    && current.device == old_record.device
                    && current.inode == old_record.inode
                    && matches
                    && fs::symlink_metadata(&path)
                        .is_ok_and(|after| Record::from(&after) == current)
                {
                    node.modified = current.modified as f64 + current.nanos as f64 / 1e9;
                    index.records.insert(path.clone(), current);
                }
            }
        }
        index.nodes.insert(path, Arc::new(node));
    }
    index.root = index.nodes[&index.root.path].clone();
}

fn missing_cached_items(index: &Index, roots: &[String]) -> Vec<String> {
    let mut missing = vec![];
    let mut pending: Vec<_> = roots
        .iter()
        .filter_map(|path| index.nodes.get(path).cloned())
        .collect();
    while let Some(node) = pending.pop() {
        match fs::symlink_metadata(&node.path) {
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && node.path != index.root.path =>
            {
                missing.push(node.path.clone())
            }
            _ => pending.extend(node.children.iter().cloned()),
        }
    }
    missing
}
fn cached_trash_roots(index: &Index) -> Vec<String> {
    if cfg!(target_os = "macos") {
        let uid = unsafe { libc::getuid() }.to_string();
        let mut roots = vec![home().join(".Trash").to_string_lossy().into_owned()];
        roots.extend(
            index
                .nodes
                .values()
                .filter(|node| {
                    node.directory
                        && node.name == uid
                        && Path::new(&node.path)
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|name| name == ".Trashes")
                })
                .map(|node| node.path.clone()),
        );
        if roots.iter().any(|root| inside(&index.root.path, root)) {
            roots.push(index.root.path.clone());
        }
        roots
    } else {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"));
        let root = data.join("Trash").to_string_lossy().into_owned();
        if inside(&index.root.path, &root) {
            vec![index.root.path.clone()]
        } else {
            vec![root]
        }
    }
}

fn removal_snapshot(data: &Value, mut response: Value, completed: &[String]) -> Result<Value> {
    let mut active = INDEX.lock().map_err(io)?;
    let Some(current) = active
        .as_mut()
        .filter(|index| data["id"].as_str() == Some(index.id.as_str()))
    else {
        return Ok(response);
    };
    let index = Arc::make_mut(current);
    if response["completed"].as_bool() == Some(true) {
        // Empty Trash can remove paths already present in the snapshot. Check
        // those known entries only; do not discover or rescan any directories.
        let missing = missing_cached_items(index, &cached_trash_roots(index));
        prune_removed(index, &missing);
    } else {
        prune_removed(index, completed);
    }
    let mut snapshot = json!({"id":index.id,"root":index.root.shallow()});
    match disk_usage(Path::new(&index.root.path)) {
        Ok((total, free)) => {
            snapshot["total"] = json!(total);
            snapshot["free"] = json!(free);
        }
        Err(error) => {
            response["warning"] = json!(format!("Disk free space could not be refreshed: {error}"))
        }
    }
    response["snapshot"] = snapshot;
    Ok(response)
}

fn index(data: &Value) -> Result<Arc<Index>> {
    INDEX
        .lock()
        .map_err(io)?
        .as_ref()
        .filter(|s| data["id"].as_str() == Some(&s.id))
        .cloned()
        .ok_or_else(|| "Scan expired. Scan the folder again.".into())
}
pub(super) fn request(route: &str, body: &str) -> Result<String> {
    let data: Value = serde_json::from_str(body).map_err(io)?;
    let response = match route {
        "/scan" => {
            let target = data["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| std::env::var("DISKTREE_ROOT").ok())
                .unwrap_or_else(|| saved_location(&state_file()).to_string_lossy().into_owned());
            let progress = Arc::new(Mutex::new(Value::Null));
            let token = data["progressToken"].as_str().unwrap_or("").to_owned();
            *PROGRESS.lock().map_err(io)? = Some((token, progress.clone()));
            let (index, response) = scan_with_progress(&absolute(&target)?, Some(progress))?;
            save_location(&index.root.path, &state_file())?;
            *INDEX.lock().map_err(io)? = Some(index);
            response
        }
        "/scan-progress" => {
            let active = PROGRESS.lock().map_err(io)?;
            match active.as_ref() {
                Some((token, progress))
                    if data["progressToken"].as_str() == Some(token.as_str()) =>
                {
                    progress.lock().map_err(io)?.clone()
                }
                _ => Value::Null,
            }
        }
        "/view" => {
            let response = view(index(&data)?.as_ref(), &data)?;
            save_location(string(&response["node"], "path")?, &state_file())?;
            response
        }
        "/delete" | "/trash" => {
            let response = remove(index(&data)?.as_ref(), &data, route == "/delete")?;
            let completed: Vec<String> = response["completed"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|path| path.as_str().map(String::from))
                .collect();
            removal_snapshot(&data, response, &completed)?
        }
        "/empty-trash" => {
            execute(&mut empty_command())?;
            removal_snapshot(&data, json!({"completed":true}), &[])?
        }
        _ => return Err("Unknown filesystem operation".into()),
    };
    serde_json::to_string(&response).map_err(io)
}

#[cfg(test)]
mod tests;
