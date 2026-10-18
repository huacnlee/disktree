use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fs::{self, Metadata},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

type Result<T> = std::result::Result<T, String>;
const PAGE_SIZE: usize = 80;
const MAP_BUDGET: usize = 240;
static INDEX: Mutex<Option<Arc<Index>>> = Mutex::new(None);
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
#[derive(Debug)]
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
struct Index {
    id: String,
    root: Arc<Node>,
    nodes: HashMap<String, Arc<Node>>,
    records: HashMap<String, Record>,
}
struct Walker {
    device: u64,
    seen: HashSet<(u64, u64)>,
    nodes: HashMap<String, Arc<Node>>,
    records: HashMap<String, Record>,
    errors: Vec<String>,
    error_count: usize,
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
        data.f_blocks.saturating_mul(block),
        data.f_bavail.saturating_mul(block),
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
    fn walk(&mut self, path: &Path) -> Result<Arc<Node>> {
        let text = path
            .to_str()
            .ok_or("Path is not valid UTF-8 (skipped)")?
            .to_owned();
        let meta = fs::symlink_metadata(path).map_err(io)?;
        self.records.insert(text.clone(), Record::from(&meta));
        let mut node = Node {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".into()),
            path: text.clone(),
            directory: meta.is_dir(),
            size: 0,
            apparent: 0,
            files: 0,
            modified: meta.mtime() as f64 + meta.mtime_nsec() as f64 / 1e9,
            children: vec![],
        };
        if meta.dev() == self.device && !meta.file_type().is_symlink() {
            if meta.is_file() {
                node.files = 1;
                if self.seen.insert((meta.dev(), meta.ino())) {
                    node.size = meta.blocks() * 512;
                    node.apparent = meta.len();
                }
            } else if meta.is_dir() {
                if cloud_only(&meta) {
                    self.error(format!("{text}: cloud-only (skipped)"));
                } else {
                    match fs::read_dir(path) {
                        Err(e) => self.error(format!("{text}: {e}")),
                        Ok(entries) => {
                            for entry in entries {
                                match entry.map_err(io).and_then(|e| self.walk(&e.path())) {
                                    Ok(child) => {
                                        node.size += child.size;
                                        node.apparent += child.apparent;
                                        node.files += child.files;
                                        node.children.push(child);
                                    }
                                    Err(e) => self.error(format!("{text}: {e}")),
                                }
                            }
                        }
                    }
                }
                node.children
                    .sort_by(|a, b| b.size.cmp(&a.size).then(a.name.cmp(&b.name)));
            }
        }
        let node = Arc::new(node);
        self.nodes.insert(text, node.clone());
        Ok(node)
    }
}
fn scan(path: &Path) -> Result<(Arc<Index>, Value)> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Choose a directory, not a symbolic link".into());
    }
    let mut walker = Walker {
        device: meta.dev(),
        seen: HashSet::new(),
        nodes: HashMap::new(),
        records: HashMap::new(),
        errors: vec![],
        error_count: 0,
    };
    let root = walker.walk(path)?;
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
    let mut unique = HashSet::new();
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
    let mut completed = vec![];
    for path in paths {
        let result = if permanent {
            let meta = fs::symlink_metadata(&path).map_err(io)?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                fs::remove_dir_all(&path).map_err(io)
            } else {
                fs::remove_file(&path).map_err(io)
            }
        } else {
            execute(&mut trash_command(&path))
        };
        if let Err(error) = result {
            return Ok(json!({"completed":completed,"error":error}));
        }
        completed.push(path);
    }
    Ok(json!({"completed":completed}))
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
            let (index, response) = scan(&absolute(&target)?)?;
            save_location(&index.root.path, &state_file())?;
            *INDEX.lock().map_err(io)? = Some(index);
            response
        }
        "/view" => {
            let response = view(index(&data)?.as_ref(), &data)?;
            save_location(string(&response["node"], "path")?, &state_file())?;
            response
        }
        "/delete" => remove(index(&data)?.as_ref(), &data, true)?,
        "/trash" => remove(index(&data)?.as_ref(), &data, false)?,
        "/empty-trash" => {
            execute(&mut empty_command())?;
            json!({"completed":true})
        }
        _ => return Err("Unknown filesystem operation".into()),
    };
    serde_json::to_string(&response).map_err(io)
}

#[cfg(test)]
mod tests;
