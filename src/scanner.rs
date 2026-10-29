use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::{Value, json};
use std::{
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
type DirectoryEntries<'a> = Box<dyn Iterator<Item = Result<(PathBuf, ScanMetadata)>> + 'a>;
fn directory_entries(path: &Path, bulk: bool) -> Result<DirectoryEntries<'_>> {
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

mod index;
use index::{Index, NO_PARENT, Node, NodeId, Totals};

struct Walker {
    device: u64,
    index: Index,
    errors: Vec<String>,
    error_count: usize,
    progress: Option<Arc<Mutex<Value>>>,
    last_progress: Instant,
    directories: u64,
    files: u64,
    bytes: u64,
    current_directory: String,
    percent: u8,
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
        if let Some(progress) = &self.progress
            && let Ok(mut value) = progress.lock()
        {
            *value = json!({"path":self.current_directory,"directories":self.directories,"files":self.files,"bytes":self.bytes,"errors":self.error_count,"percent":self.percent});
        }
        self.last_progress = Instant::now();
    }
    fn new(device: u64, root: &str, progress: Option<Arc<Mutex<Value>>>) -> Result<Self> {
        Ok(Self {
            device,
            index: Index::new(root)?,
            errors: vec![],
            error_count: 0,
            progress,
            last_progress: Instant::now() - Duration::from_millis(100),
            directories: 0,
            files: 0,
            bytes: 0,
            current_directory: String::new(),
            percent: 0,
        })
    }
    fn add(&mut self, path: &Path, meta: ScanMetadata, parent: NodeId) -> Result<NodeId> {
        let name = path
            .file_name()
            .map(|n| n.to_str().expect("validated UTF-8 path"))
            .unwrap_or("/")
            .to_owned();
        let mut node = Node {
            record: meta.record.clone(),
            name,
            parent,
            directory: meta.directory,
            linked: meta.links > 1 && meta.file,
            size: 0,
            apparent: 0,
            files: 0,
        };
        if meta.record.device == self.device && !meta.symlink {
            if meta.file {
                self.files += 1;
                let parent = path.parent().unwrap_or(path).to_string_lossy();
                if self.current_directory != parent {
                    self.current_directory = parent.into_owned();
                }
                node.files = 1;
                if meta.links <= 1
                    || self
                        .index
                        .find_link(meta.record.device, meta.record.inode)?
                        .is_none()
                {
                    node.size = meta.allocated;
                    node.apparent = meta.record.size;
                    self.bytes += node.size;
                }
            } else if meta.directory {
                self.directories += 1;
                self.current_directory = path.to_string_lossy().into_owned();
                if meta.evicted {
                    self.error(format!("{}: cloud-only (skipped)", path.display()));
                }
            }
        }
        let id = self.index.insert(node)?;
        self.publish_progress(false);
        Ok(id)
    }
    #[cfg(test)]
    fn collect(
        &mut self,
        path: PathBuf,
        meta: ScanMetadata,
        directories: &mut HashMap<PathBuf, NodeId>,
    ) -> Result<()> {
        let parent = if self.index.live == 0 {
            NO_PARENT
        } else {
            *directories
                .get(path.parent().ok_or("Missing scan parent")?)
                .ok_or("Missing scan parent")?
        };
        let directory = meta.directory;
        let id = self.add(&path, meta, parent)?;
        if directory {
            directories.insert(path, id);
        }
        Ok(())
    }
}
struct DirectoryQueue {
    db: rusqlite::Connection,
    pending: u64,
    active: u64,
    completed: u64,
    cancelled: bool,
    root_depth: usize,
    // Directories queued and still waiting, by depth below the root.
    queued: Vec<u64>,
    waiting: Vec<u64>,
}
impl DirectoryQueue {
    fn new(root: &Path) -> Result<Self> {
        let db = index::temporary_database(1024)?;
        db.execute_batch("CREATE TABLE queue (id INTEGER PRIMARY KEY,path TEXT NOT NULL); BEGIN;")
            .map_err(io)?;
        let mut queue = Self {
            db,
            pending: 0,
            active: 0,
            completed: 0,
            cancelled: false,
            root_depth: root.components().count(),
            queued: vec![],
            waiting: vec![],
        };
        queue.push(0, root)?;
        Ok(queue)
    }
    fn push(&mut self, id: NodeId, path: &Path) -> Result<()> {
        self.db
            .prepare_cached("INSERT INTO queue VALUES (?1,?2)")
            .map_err(io)?
            .execute(rusqlite::params![
                id,
                path.to_str().ok_or("Invalid scan path")?
            ])
            .map_err(io)?;
        self.pending += 1;
        let depth = self.depth(path);
        if self.queued.len() <= depth {
            self.queued.resize(depth + 1, 0);
            self.waiting.resize(depth + 1, 0);
        }
        self.queued[depth] += 1;
        self.waiting[depth] += 1;
        Ok(())
    }
    fn pop(&mut self) -> Result<Option<(NodeId, PathBuf)>> {
        use rusqlite::OptionalExtension;
        let next: Option<(NodeId, String)> = self
            .db
            .prepare_cached("SELECT id,path FROM queue ORDER BY id DESC LIMIT 1")
            .map_err(io)?
            .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()
            .map_err(io)?;
        if let Some((id, path)) = next {
            self.db
                .prepare_cached("DELETE FROM queue WHERE id=?1")
                .map_err(io)?
                .execute([id])
                .map_err(io)?;
            self.pending -= 1;
            self.active += 1;
            let path = PathBuf::from(path);
            let depth = self.depth(&path);
            self.waiting[depth] -= 1;
            Ok(Some((id, path)))
        } else {
            Ok(None)
        }
    }
    fn depth(&self, path: &Path) -> usize {
        path.components().count().saturating_sub(self.root_depth)
    }
    /// Directories still to read. The queue is depth-first, so what waits
    /// is mostly unopened siblings near the root; each stands for its whole
    /// subtree, sized like the subtrees already opened at its depth.
    fn remaining(&self) -> f64 {
        let mut deeper = 0u64;
        let mut remaining = self.active as f64;
        for depth in (0..self.queued.len()).rev() {
            let waiting = self.waiting[depth];
            let opened = self.queued[depth] - waiting;
            if waiting > 0 {
                remaining += waiting as f64 * (1. + deeper as f64 / opened.max(1) as f64);
            }
            deeper += self.queued[depth];
        }
        remaining
    }
}
// Directory discovery changes the denominator; keep the estimate monotonic and
// reserve 100% for the fully reduced and assembled tree.
fn scan_percent(previous: u8, completed: u64, remaining: f64) -> u8 {
    let total = completed as f64 + remaining;
    let estimate = if total <= 0. {
        0
    } else {
        (completed as f64 / total * 100.) as u8
    };
    previous.max(estimate.min(99))
}
type Entries = Vec<(PathBuf, Result<ScanMetadata>)>;

// A fixed number of directory readers feeds bounded batches to one reducer.
// Only the reducer owns the index, hard-link set, and progress counters.
fn read_directories(path: &Path, workers: usize, bulk: bool, walker: &mut Walker) -> Result<()> {
    enum Event {
        Entries(NodeId, Entries),
        Done,
        Error(String),
    }
    let queue = (Mutex::new(DirectoryQueue::new(path)?), Condvar::new());
    let (sender, receiver) = mpsc::sync_channel::<Event>(workers * 2);
    let device = walker.device;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let (parent, directory) = {
                        let mut state = queue.0.lock().unwrap();
                        loop {
                            if state.cancelled {
                                return;
                            }
                            match state.pop() {
                                Ok(Some(next)) => break next,
                                Ok(None) if state.active == 0 => return,
                                Ok(None) => state = queue.1.wait(state).unwrap(),
                                Err(error) => {
                                    drop(state);
                                    let _ = sender.send(Event::Error(error));
                                    return;
                                }
                            }
                        }
                    };
                    let mut batch = Vec::with_capacity(128);
                    match directory_entries(&directory, bulk) {
                        Err(error) => batch.push((directory.clone(), Err(error))),
                        Ok(entries) => {
                            for entry in entries {
                                let entry = match entry {
                                    Ok((path, meta)) if path.to_str().is_some() => (path, Ok(meta)),
                                    Ok((path, _)) => {
                                        (path, Err("Path is not valid UTF-8 (skipped)".into()))
                                    }
                                    Err(error) => (directory.clone(), Err(error)),
                                };
                                batch.push(entry);
                                if batch.len() == 128
                                    && sender
                                        .send(Event::Entries(
                                            parent,
                                            std::mem::replace(&mut batch, Vec::with_capacity(128)),
                                        ))
                                        .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    if !batch.is_empty() && sender.send(Event::Entries(parent, batch)).is_err() {
                        return;
                    }
                    if sender.send(Event::Done).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let result = (|| {
            for event in receiver {
                match event {
                    Event::Error(error) => return Err(error),
                    Event::Done => {
                        // Completion is reduced after all that directory's
                        // batches, so readers cannot exit before children have
                        // been assigned IDs and added to the disk queue.
                        let mut state = queue.0.lock().map_err(io)?;
                        state.active -= 1;
                        state.completed += 1;
                        queue.1.notify_all();
                    }
                    Event::Entries(parent, batch) => {
                        for (path, metadata) in batch {
                            match metadata {
                                Err(error) => walker.error(format!("{}: {error}", path.display())),
                                Ok(meta) => {
                                    let descend = meta.directory
                                        && meta.record.device == device
                                        && !meta.symlink
                                        && !meta.evicted;
                                    let id = walker.add(&path, meta, parent)?;
                                    if descend {
                                        queue.0.lock().map_err(io)?.push(id, &path)?;
                                        queue.1.notify_one();
                                    }
                                }
                            }
                        }
                    }
                }
                let state = queue.0.lock().map_err(io)?;
                walker.percent = scan_percent(
                    walker.percent,
                    state.completed,
                    state.remaining(),
                );
            }
            Ok(())
        })();
        if result.is_err() {
            let mut state = queue.0.lock().unwrap();
            state.cancelled = true;
            queue.1.notify_all();
        }
        result
    })
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
    let mut walker = Walker::new(meta.dev(), root_path, progress)?;
    walker.add(path, meta.into(), NO_PARENT)?;
    if !skip_root {
        read_directories(path, workers.max(1), bulk, &mut walker)?;
    }
    walker.index.finish()?;
    walker.percent = 100;
    walker.publish_progress(true);
    let (total, free) = disk_usage(path)?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    );
    let response = json!({"id":id,"root":walker.index.shallow(0)?,"total":total,"free":free,"errors":walker.errors,"errorCount":walker.error_count});
    walker.index.id = id;
    Ok((Arc::new(walker.index), response))
}

fn aggregate(total: Totals, path: String, name: String) -> Value {
    json!({"path":path,"name":name,"directory":false,"size":total.size,"apparent":total.apparent,
        "files":total.files,"modified":0,"children":[],"aggregate":true})
}
fn bounded(
    index: &Index,
    items: &[NodeId],
    total: Totals,
    depth: usize,
    levels: usize,
    budget: &mut usize,
    path: &str,
) -> Result<Vec<Value>> {
    let selected = items.len().min(budget.saturating_sub(1));
    *budget -= selected;
    let weights = index.weights(&items[..selected])?;
    let rest = total.minus(weights);
    let mut output = items[..selected]
        .iter()
        .map(|id| index.shallow(*id))
        .collect::<Result<Vec<_>>>()?;
    if rest.count > 0 {
        *budget -= 1;
        output.push(aggregate(
            rest,
            format!("{path}/#remaining-{depth}-{budget}"),
            format!("{} more items", rest.count),
        ));
    }
    for (item, id) in output[..selected].iter_mut().zip(items) {
        if index.node(*id)?.directory && depth < levels && *budget > 1 {
            let (children, total) =
                index.listing(*id, true, "", "size", 0, budget.saturating_sub(1))?;
            item["children"] = json!(bounded(
                index,
                &children,
                total,
                depth + 1,
                levels,
                budget,
                &index.path(*id)?
            )?);
        }
    }
    Ok(output)
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().ok_or_else(|| format!("Missing {key}"))
}
fn inside(path: &str, parent: &str) -> bool {
    path != parent && Path::new(path).starts_with(parent)
}
fn view(index: &Index, data: &Value) -> Result<Value> {
    let _access = index.access.lock().map_err(io)?;
    let mut path = string(data, "path")?.to_owned();
    let metric = data["metric"].as_str().unwrap_or("size");
    if !["size", "apparent", "files"].contains(&metric) {
        return Err("Invalid metric".into());
    }
    if data["fallback"].as_bool().unwrap_or(false) {
        if path != index.root_path && !inside(&path, &index.root_path) {
            return Err("Directory is outside the scan".into());
        }
        while index.lookup(&path)?.is_none() && path != index.root_path {
            path = Path::new(&path)
                .parent()
                .ok_or("Directory is outside the scan")?
                .to_string_lossy()
                .into_owned();
        }
    }
    let id = index
        .lookup(&path)?
        .ok_or("Directory no longer exists. Scan again.")?;
    if !index.node(id)?.directory {
        return Err("Choose a directory".into());
    }
    let query = data["query"].as_str().unwrap_or("").to_lowercase();
    let hidden = data["hidden"].as_bool().unwrap_or(true);
    let requested = data["offset"].as_u64().unwrap_or(0) as usize;
    let (page, total) = index.listing(id, hidden, &query, metric, requested, PAGE_SIZE)?;
    let offset = requested.min(total.count.saturating_sub(1) / PAGE_SIZE * PAGE_SIZE);
    let levels = data["levels"].as_u64().unwrap_or(2).clamp(1, 3) as usize;
    let mut budget = MAP_BUDGET;
    let page_weights = index.weights(&page)?;
    let mut tiles = bounded(index, &page, page_weights, 1, levels, &mut budget, &path)?;
    let rest = total.minus(page_weights);
    if rest.count > 0 {
        tiles.push(aggregate(
            rest,
            format!("{path}/#other-pages"),
            format!("{} items on other pages", rest.count),
        ));
    }
    let mut ancestors = vec![];
    let mut cursor = id;
    loop {
        ancestors.push(index.shallow(cursor)?);
        if cursor == 0 {
            break;
        }
        cursor = index.node(cursor)?.parent;
    }
    ancestors.reverse();
    Ok(
        json!({"node":index.shallow(id)?,"children":page.iter().map(|id|index.shallow(*id)).collect::<Result<Vec<_>>>()?,
        "map":tiles,"ancestors":ancestors,"offset":offset,"count":total.count,"pageSize":PAGE_SIZE}),
    )
}
fn validate_removal(index: &Index, paths: &[String], permanent: bool) -> Result<()> {
    if paths.is_empty() {
        return Err("Nothing marked".into());
    }
    for path in paths {
        let id = index
            .lookup(path)?
            .filter(|id| *id != 0)
            .ok_or("Only scanned descendants can be removed")?;
        if paths.iter().any(|other| inside(path, other)) {
            return Err("Overlapping selections".into());
        }
        let current = fs::symlink_metadata(path).map_err(io)?;
        if Record::from(&current) != index.node(id)?.record {
            return Err("Selection changed since scan. Scan again before removing it.".into());
        }
        let mut parent = index.node(id)?.parent;
        loop {
            let meta = fs::symlink_metadata(index.path(parent)?).map_err(io)?;
            let old = index.node(parent)?.record;
            if meta.file_type().is_symlink() || meta.dev() != old.device || meta.ino() != old.inode
            {
                return Err("Directory changed. Scan again.".into());
            }
            if parent == 0 {
                break;
            }
            parent = index.node(parent)?.parent;
        }
        let root_device = index.root()?.record.device;
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let old = index.node(id)?.record;
            if permanent && old.device != root_device {
                return Err("Cannot permanently delete across mounted volumes".into());
            }
            let meta = fs::symlink_metadata(index.path(id)?).map_err(io)?;
            if Record::from(&meta) != old {
                return Err("Contents changed since scan. Scan again before removing them.".into());
            }
            pending.extend(index.children(id)?);
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
// Removal progress is published under the request's token, as scans are.
// Totals come from the scan; permanent deletion reports each unlinked file.
struct RemovalProgress {
    progress: Option<Arc<Mutex<Value>>>,
    last: Instant,
    items: usize,
    total_items: usize,
    files: u64,
    total_files: u64,
    bytes: u64,
    total_bytes: u64,
    current: String,
}
impl RemovalProgress {
    fn new(progress: Option<Arc<Mutex<Value>>>, totals: &[(u64, u64)]) -> Self {
        Self {
            progress,
            last: Instant::now() - Duration::from_millis(100),
            items: 0,
            total_items: totals.len(),
            files: 0,
            total_files: totals.iter().map(|t| t.0).sum(),
            bytes: 0,
            total_bytes: totals.iter().map(|t| t.1).sum(),
            current: String::new(),
        }
    }
    fn percent(&self) -> u8 {
        let fraction = if self.total_bytes > 0 {
            self.bytes as f64 / self.total_bytes as f64
        } else if self.total_files > 0 {
            self.files as f64 / self.total_files as f64
        } else {
            self.items as f64 / self.total_items.max(1) as f64
        };
        (fraction.clamp(0., 1.) * 100.) as u8
    }
    fn publish(&mut self, force: bool) {
        if !force && self.last.elapsed() < Duration::from_millis(50) {
            return;
        }
        if let Some(progress) = &self.progress
            && let Ok(mut value) = progress.lock()
        {
            *value = json!({"percent":self.percent(),"items":self.items,"totalItems":self.total_items,
                "files":self.files.min(self.total_files),"totalFiles":self.total_files,
                "bytes":self.bytes.min(self.total_bytes),"totalBytes":self.total_bytes,"current":self.current});
        }
        self.last = Instant::now();
    }
}
fn publish_progress(data: &Value) -> Result<Arc<Mutex<Value>>> {
    let progress = Arc::new(Mutex::new(Value::Null));
    let token = data["progressToken"].as_str().unwrap_or("").to_owned();
    *PROGRESS.lock().map_err(io)? = Some((token, progress.clone()));
    Ok(progress)
}
fn remove(index: &Index, data: &Value, permanent: bool) -> Result<Value> {
    remove_with_progress(index, data, permanent, Some(publish_progress(data)?))
}
fn remove_with_progress(
    index: &Index,
    data: &Value,
    permanent: bool,
    published: Option<Arc<Mutex<Value>>>,
) -> Result<Value> {
    let _access = index.access.lock().map_err(io)?;
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
    let mut totals = vec![];
    for path in &paths {
        let node = index.node(
            index
                .lookup(path)?
                .ok_or("Only scanned descendants can be removed")?,
        )?;
        totals.push((node.files.max(u64::from(!node.directory)), node.size));
    }
    let device = index.root()?.record.device;
    let mut progress = RemovalProgress::new(published, &totals);
    progress.publish(true);
    let mut item = 0;
    let result = execute_removals(paths, |path| {
        let (base_files, base_bytes) = (progress.files, progress.bytes);
        let (files, bytes) = totals[item];
        progress.current = path.to_owned();
        progress.publish(true);
        let result = if permanent {
            remove_tree(path, device, &mut |size| {
                progress.files = (progress.files + 1).min(base_files + files);
                progress.bytes = (progress.bytes + size).min(base_bytes + bytes);
                progress.publish(false);
            })
        } else {
            execute(&mut trash_command(path))
        };
        if result.is_ok() {
            item += 1;
            progress.items = item;
            progress.files = base_files + files;
            progress.bytes = base_bytes + bytes;
            progress.publish(true);
        }
        result
    });
    progress.current.clear();
    progress.publish(true);
    result
}
/// Removes `path` without following symbolic links or leaving `device`.
/// Directories are opened relative to their parent with `O_NOFOLLOW`, so a
/// directory swapped for a link mid-removal is never traversed.
fn remove_tree(path: &str, device: u64, on_file: &mut dyn FnMut(u64)) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        fs::remove_file(path).map_err(io)?;
        on_file(meta.blocks() * 512);
        return Ok(());
    }
    let name = CString::new(Path::new(path).as_os_str().as_bytes()).map_err(io)?;
    // SAFETY: `name` is a valid C string; the descriptor is owned by `Dir`.
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    let dir = Dir::open(fd, device, meta.ino())?;
    remove_contents(dir, device, on_file)?;
    fs::remove_dir(path).map_err(io)
}
struct Dir(*mut libc::DIR);
impl Dir {
    /// Takes ownership of `fd`, checking it is still the expected directory.
    fn open(fd: libc::c_int, device: u64, inode: u64) -> Result<Self> {
        if fd < 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        // SAFETY: `fd` is an open descriptor this function owns.
        unsafe {
            let mut stat = std::mem::zeroed::<libc::stat>();
            if libc::fstat(fd, &mut stat) != 0 {
                let error = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(io(error));
            }
            if stat.st_dev as u64 != device {
                libc::close(fd);
                return Err("Cannot permanently delete across mounted volumes".into());
            }
            if stat.st_ino as u64 != inode {
                libc::close(fd);
                return Err("Directory changed. Scan again.".into());
            }
            let dir = libc::fdopendir(fd);
            if dir.is_null() {
                let error = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(io(error));
            }
            Ok(Self(dir))
        }
    }
    fn fd(&self) -> libc::c_int {
        // SAFETY: the stream stays open while `self` lives.
        unsafe { libc::dirfd(self.0) }
    }
    fn names(&self) -> Result<Vec<CString>> {
        let mut names = vec![];
        loop {
            // SAFETY: readdir reports errors through errno, cleared first.
            unsafe {
                *errno() = 0;
                let entry = libc::readdir(self.0);
                if entry.is_null() {
                    if *errno() != 0 {
                        return Err(io(std::io::Error::last_os_error()));
                    }
                    return Ok(names);
                }
                let name = std::ffi::CStr::from_ptr((*entry).d_name.as_ptr());
                if name.to_bytes() != b"." && name.to_bytes() != b".." {
                    names.push(name.to_owned());
                }
            }
        }
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        // SAFETY: the stream was opened by `Dir::open` and is closed once.
        unsafe { libc::closedir(self.0) };
    }
}
#[cfg(target_os = "linux")]
unsafe fn errno() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}
#[cfg(target_os = "macos")]
unsafe fn errno() -> *mut libc::c_int {
    unsafe { libc::__error() }
}
fn remove_contents(dir: Dir, device: u64, on_file: &mut dyn FnMut(u64)) -> Result<()> {
    // Read the whole directory before unlinking: removing entries while a
    // stream is open leaves later readdir results unspecified.
    let names = dir.names()?;
    let fd = dir.fd();
    for name in names {
        // SAFETY: `fd` is open for the loop and `name` is a valid C string.
        unsafe {
            let mut stat = std::mem::zeroed::<libc::stat>();
            if libc::fstatat(fd, name.as_ptr(), &mut stat, libc::AT_SYMLINK_NOFOLLOW) != 0 {
                return Err(io(std::io::Error::last_os_error()));
            }
            if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
                let child = libc::openat(
                    fd,
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                );
                remove_contents(
                    Dir::open(child, device, stat.st_ino as u64)?,
                    device,
                    on_file,
                )?;
                if libc::unlinkat(fd, name.as_ptr(), libc::AT_REMOVEDIR) != 0 {
                    return Err(io(std::io::Error::last_os_error()));
                }
            } else {
                if libc::unlinkat(fd, name.as_ptr(), 0) != 0 {
                    return Err(io(std::io::Error::last_os_error()));
                }
                on_file(stat.st_blocks as u64 * 512);
            }
        }
    }
    Ok(())
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

fn mark_ancestors(index: &Index, id: NodeId, dirty: &mut HashSet<NodeId>) -> Result<()> {
    let mut parent = index.node(id)?.parent;
    while parent != NO_PARENT {
        dirty.insert(parent);
        parent = index.node(parent)?.parent;
    }
    Ok(())
}
fn prune_removed(index: &mut Index, completed: &[String]) -> Result<()> {
    let access = index.access.clone();
    let _access = access.lock().map_err(io)?;
    index.db()?.execute_batch("BEGIN;").map_err(io)?;
    let original_live = index.live;
    let result = prune_entries(index, completed);
    if result.is_err() {
        index.live = original_live;
        let _ = index.db()?.execute_batch("ROLLBACK;");
    } else {
        index
            .db()?
            .execute_batch("COMMIT; PRAGMA shrink_memory;")
            .map_err(io)?;
    }
    result
}
fn prune_entries(index: &mut Index, completed: &[String]) -> Result<()> {
    let mut dirty = HashSet::default();
    let mut parents = HashSet::default();
    let mut lost_owners = HashMap::default();
    let mut pending = vec![];
    for path in completed {
        if let Some(id) = index.lookup(path)?.filter(|id| *id != 0) {
            mark_ancestors(index, id, &mut dirty)?;
            parents.insert(index.node(id)?.parent);
            pending.push(id);
        }
    }
    while let Some(id) = pending.pop() {
        let Some(node) = index.node_optional(id)? else {
            continue;
        };
        if node.linked && (node.size > 0 || node.apparent > 0) {
            lost_owners.insert(
                (node.record.device, node.record.inode),
                (node.size, node.apparent),
            );
        }
        pending.extend(index.children(id)?);
        index.delete(id)?;
    }
    for ((device, inode), (size, apparent)) in lost_owners {
        if let Some(id) = index.find_link(device, inode)? {
            index.set_weights(id, size, apparent)?;
            mark_ancestors(index, id, &mut dirty)?;
        }
    }
    let mut dirty: Vec<_> = dirty.into_iter().collect();
    dirty.sort_unstable_by(|a, b| b.cmp(a));
    for id in dirty {
        if index.node_optional(id)?.is_none() {
            continue;
        }
        index.reduce(id)?;
        if parents.contains(&id) {
            refresh_parent_record(index, id)?;
        }
    }
    Ok(())
}
fn refresh_parent_record(index: &Index, id: NodeId) -> Result<()> {
    let path = index.path(id)?;
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Ok(());
    };
    let current = Record::from(&meta);
    let old = index.node(id)?.record;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || current.device != old.device
        || current.inode != old.inode
    {
        return Ok(());
    }
    let (_, total) = index.listing(id, true, "", "size", 0, 0)?;
    let Ok(entries) = fs::read_dir(&path) else {
        return Ok(());
    };
    let mut count = 0;
    for entry in entries {
        let Ok(entry) = entry else { return Ok(()) };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Ok(());
        };
        if index.child_named(id, name)?.is_none() {
            return Ok(());
        }
        count += 1;
    }
    if count == total.count
        && fs::symlink_metadata(&path).is_ok_and(|after| Record::from(&after) == current)
    {
        index.set_record(id, &current)?;
    }
    Ok(())
}
fn missing_cached_items(index: &Index, roots: &[String]) -> Result<Vec<String>> {
    let mut missing = vec![];
    let mut pending = vec![];
    for path in roots {
        if let Some(id) = index.lookup(path)? {
            pending.push(id);
        }
    }
    while let Some(id) = pending.pop() {
        let path = index.path(id)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && id != 0 => {
                missing.push(path)
            }
            _ => pending.extend(index.children(id)?),
        }
    }
    Ok(missing)
}
fn cached_trash_roots(index: &Index) -> Result<Vec<String>> {
    if cfg!(target_os = "macos") {
        let uid = unsafe { libc::getuid() }.to_string();
        let mut roots = vec![home().join(".Trash").to_string_lossy().into_owned()];
        for id in index.trash_directories(&uid)? {
            let path = index.path(id)?;
            if Path::new(&path)
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == ".Trashes")
            {
                roots.push(path);
            }
        }
        if roots.iter().any(|root| inside(&index.root_path, root)) {
            roots.push(index.root_path.clone());
        }
        Ok(roots)
    } else {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"));
        let root = data.join("Trash").to_string_lossy().into_owned();
        Ok(if inside(&index.root_path, &root) {
            vec![index.root_path.clone()]
        } else {
            vec![root]
        })
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
        let missing = missing_cached_items(index, &cached_trash_roots(index)?)?;
        prune_removed(index, &missing)?;
    } else {
        prune_removed(index, completed)?;
    }
    let mut snapshot = json!({"id":index.id,"root":index.shallow(0)?});
    match disk_usage(Path::new(index.root_path.as_str())) {
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
#[cfg(test)]
pub(super) fn request(route: &str, body: &str) -> Result<String> {
    let data: Value = serde_json::from_str(body).map_err(io)?;
    serde_json::to_string(&dispatch(route, &data)?).map_err(io)
}

pub(super) fn dispatch(route: &str, data: &Value) -> Result<Value> {
    let response = match route {
        "/scan" => {
            let target = data["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| std::env::var("DISKTREE_ROOT").ok())
                .unwrap_or_else(|| saved_location(&state_file()).to_string_lossy().into_owned());
            let progress = publish_progress(data)?;
            let (index, response) = scan_with_progress(&absolute(&target)?, Some(progress))?;
            save_location(&index.root_path, &state_file())?;
            *INDEX.lock().map_err(io)? = Some(index);
            response
        }
        "/progress" | "/scan-progress" => {
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
            let response = view(index(data)?.as_ref(), data)?;
            save_location(string(&response["node"], "path")?, &state_file())?;
            response
        }
        "/delete" | "/trash" => {
            let response = remove(index(data)?.as_ref(), data, route == "/delete")?;
            let completed: Vec<String> = response["completed"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|path| path.as_str().map(String::from))
                .collect();
            removal_snapshot(data, response, &completed)?
        }
        "/empty-trash" => {
            *publish_progress(data)?.lock().map_err(io)? =
                json!({"percent":0,"items":0,"totalItems":0});
            execute(&mut empty_command())?;
            removal_snapshot(data, json!({"completed":true}), &[])?
        }
        _ => return Err("Unknown filesystem operation".into()),
    };
    Ok(response)
}

#[cfg(test)]
mod tests;
