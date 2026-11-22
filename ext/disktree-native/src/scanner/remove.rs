//! Permanent removal of a directory tree.
//!
//! Every directory is opened relative to its parent with `O_NOFOLLOW` and
//! checked against the device and inode it was listed with, so a directory
//! swapped for a link, or a volume mounted below the tree, is never entered.
//!
//! `readdir` gives each entry's type, so a file is unlinked without being
//! stated first. Directories are still stated, as they must be checked
//! before they are entered.
//!
//! On solid-state storage several workers remove separate subtrees at once;
//! a directory is removed by whichever worker finishes its last child.
//! Elsewhere one thread removes the tree, each directory in inode order.

use super::{Result, io};
use std::{
    ffi::{CStr, CString},
    fs,
    os::{
        fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd},
        unix::fs::MetadataExt,
    },
    sync::{
        Arc, Condvar, Mutex, Once,
        atomic::{AtomicUsize, Ordering},
    },
};

const CROSSED: &str = "Cannot permanently delete across mounted volumes";
/// Files unlinked between progress reports.
const BATCH: u64 = 256;

/// What part of a removal is done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Removed {
    pub files: u64,
    /// Bytes held by the files whose sizes the listing gave.
    pub bytes: u64,
    /// Files whose sizes the listing did not give; not in `bytes`.
    pub unmeasured: u64,
}
impl Removed {
    fn add(&mut self, bytes: Option<u64>) {
        self.files += 1;
        match bytes {
            Some(bytes) => self.bytes += bytes,
            None => self.unmeasured += 1,
        }
    }
}
type Report<'a> = &'a (dyn Fn(Removed) + Sync);

/// Removes `path` without following symbolic links or leaving `device`,
/// with `workers` threads; one keeps the removal on the calling thread.
pub(super) fn remove_tree(path: &str, device: u64, workers: usize, report: Report) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_dir() {
        fs::remove_file(path).map_err(io)?;
        report(Removed {
            files: 1,
            bytes: meta.blocks() * 512,
            unmeasured: 0,
        });
        return Ok(());
    }
    let name = CString::new(path.as_bytes()).map_err(io)?;
    let fd = open_checked(libc::AT_FDCWD, &name, device, meta.ino())?;
    if workers > 1 {
        clear_parallel(fd, device, workers, report)?;
    } else {
        clear(&fd, device, report)?;
    }
    fs::remove_dir(path).map_err(io)
}

/// Opens directory `name` relative to `dir`, if it is still the directory
/// `inode` on `device`.
fn open_checked(dir: RawFd, name: &CStr, device: u64, inode: u64) -> Result<OwnedFd> {
    // SAFETY: `name` is a valid C string; a descriptor returned is owned here.
    let fd = unsafe {
        libc::openat(
            dir,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` was just opened and nothing else owns it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: `stat` is plain data the call fills; `fd` is open.
    let stat = unsafe {
        let mut stat = std::mem::zeroed::<libc::stat>();
        if libc::fstat(fd.as_raw_fd(), &mut stat) != 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        stat
    };
    if stat.st_dev as u64 != device {
        return Err(CROSSED.into());
    }
    if stat.st_ino as u64 != inode {
        return Err("Directory changed. Scan again.".into());
    }
    Ok(fd)
}

fn unlink(dir: BorrowedFd, name: &CStr, directory: bool) -> Result<()> {
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    // SAFETY: `dir` is open and `name` is a valid C string.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(())
}

enum Kind {
    /// A directory, with the inode it must still have when opened.
    Directory(u64),
    /// Anything unlinked without being entered, with its size when known.
    Leaf(Option<u64>),
}
struct Entry {
    name: CString,
    /// The inode number, which orders the removal; see [`ordered`].
    inode: u64,
    kind: Kind,
}

/// Every entry of the directory open as `dir`, from `readdir`, with a stat
/// only for directories and entries whose type the file system left out.
/// The whole listing is read before anything is unlinked: removing entries
/// while a listing is open leaves what it returns next unspecified.
fn list(dir: BorrowedFd) -> Result<Vec<Entry>> {
    let stream = Stream::open(dir)?;
    let mut entries = vec![];
    loop {
        // SAFETY: readdir reports errors through errno, cleared first; the
        // entry is read before the stream is used again.
        unsafe {
            *errno() = 0;
            let entry = libc::readdir(stream.0);
            if entry.is_null() {
                if *errno() != 0 {
                    return Err(io(std::io::Error::last_os_error()));
                }
                return Ok(entries);
            }
            let name = CStr::from_ptr((*entry).d_name.as_ptr());
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            let kind = match (*entry).d_type {
                libc::DT_DIR | libc::DT_UNKNOWN => {
                    let mut stat = std::mem::zeroed::<libc::stat>();
                    if libc::fstatat(
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        &mut stat,
                        libc::AT_SYMLINK_NOFOLLOW,
                    ) != 0
                    {
                        return Err(io(std::io::Error::last_os_error()));
                    }
                    if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
                        Kind::Directory(stat.st_ino as u64)
                    } else {
                        Kind::Leaf(Some(stat.st_blocks as u64 * 512))
                    }
                }
                _ => Kind::Leaf(None),
            };
            entries.push(Entry {
                name: name.to_owned(),
                inode: (*entry).d_ino as u64,
                kind,
            });
        }
    }
}

/// A `readdir` stream over a duplicate of a directory descriptor.
struct Stream(*mut libc::DIR);
impl Stream {
    fn open(dir: BorrowedFd) -> Result<Self> {
        let fd = dir.try_clone_to_owned().map_err(io)?.into_raw_fd();
        // SAFETY: `fd` is a descriptor this function owns; on success the
        // stream takes it, and on failure it is closed here.
        unsafe {
            let stream = libc::fdopendir(fd);
            if stream.is_null() {
                let error = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(io(error));
            }
            Ok(Self(stream))
        }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the stream was opened by `Stream::open` and is closed once.
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

/// The entries of `dir` in inode order. A listing comes in name-hash order,
/// unrelated to where each entry's metadata is kept; inode numbers follow
/// that place on ext4 and XFS, and key the metadata trees of APFS and
/// btrfs. Unlinking in their order walks the metadata forward instead of
/// jumping across it, which a rotational disk pays for in seeks. GNU `rm`
/// sorts large directories the same way. Solid-state storage has no seeks
/// to save, and measured no faster, so only the sequential removal sorts.
fn ordered(dir: BorrowedFd) -> Result<Vec<Entry>> {
    let mut entries = list(dir)?;
    entries.sort_unstable_by_key(|entry| entry.inode);
    Ok(entries)
}

/// Unlinks every file of the directory open as `dir`, reporting them, and
/// returns its subdirectories; in inode order when `sorted`.
fn sweep(dir: BorrowedFd, sorted: bool, report: Report) -> Result<Vec<(CString, u64)>> {
    let mut removed = Removed::default();
    let mut directories = vec![];
    let entries = if sorted { ordered(dir)? } else { list(dir)? };
    for entry in entries {
        match entry.kind {
            Kind::Directory(inode) => directories.push((entry.name, inode)),
            Kind::Leaf(bytes) => {
                unlink(dir, &entry.name, false)?;
                removed.add(bytes);
                if removed.files >= BATCH {
                    report(std::mem::take(&mut removed));
                }
            }
        }
    }
    if removed.files > 0 {
        report(removed);
    }
    Ok(directories)
}

/// Empties the directory open as `dir` on this thread, the way rotational
/// and unidentified storage is removed: in inode order.
fn clear(dir: &OwnedFd, device: u64, report: Report) -> Result<()> {
    for (name, inode) in sweep(dir.as_fd(), true, report)? {
        let child = open_checked(dir.as_raw_fd(), &name, device, inode)?;
        clear(&child, device, report)?;
        drop(child);
        unlink(dir.as_fd(), &name, true)?;
    }
    Ok(())
}

/// A directory being emptied. `pending` counts its subdirectories not yet
/// removed, plus one while its own files are being unlinked.
struct Node {
    fd: OwnedFd,
    parent: Option<(Arc<Node>, CString)>,
    pending: AtomicUsize,
}
struct Task {
    parent: Arc<Node>,
    name: CString,
    inode: u64,
}
struct Queue {
    tasks: Vec<Task>,
    active: usize,
    error: Option<String>,
}

/// Empties the directory open as `dir` with `workers` threads. Tasks are
/// taken last in, first out, so each worker goes deep before wide and the
/// directories held open stay near the workers times the tree's depth.
fn clear_parallel(dir: OwnedFd, device: u64, workers: usize, report: Report) -> Result<()> {
    raise_open_file_limit();
    let root = Arc::new(Node {
        fd: dir,
        parent: None,
        pending: AtomicUsize::new(1),
    });
    let tasks = expand(&root, report)?;
    release(root)?;
    let queue = (
        Mutex::new(Queue {
            tasks,
            active: 0,
            error: None,
        }),
        Condvar::new(),
    );
    std::thread::scope(|scope| {
        for _ in 1..workers {
            scope.spawn(|| work(&queue, device, report));
        }
        work(&queue, device, report);
    });
    let state = queue.0.into_inner().map_err(io)?;
    state.error.map_or(Ok(()), Err)
}
fn work(queue: &(Mutex<Queue>, Condvar), device: u64, report: Report) {
    let (lock, ready) = queue;
    loop {
        let task = {
            let Ok(mut state) = lock.lock() else { return };
            loop {
                if state.error.is_some() {
                    return;
                }
                if let Some(task) = state.tasks.pop() {
                    state.active += 1;
                    break task;
                }
                if state.active == 0 {
                    return;
                }
                state = match ready.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
        };
        let result = run(task, device, report);
        let Ok(mut state) = lock.lock() else { return };
        state.active -= 1;
        match result {
            Ok(tasks) if state.error.is_none() => {
                match tasks.len() {
                    0 => {}
                    1 => ready.notify_one(),
                    _ => ready.notify_all(),
                }
                state.tasks.extend(tasks);
            }
            Ok(_) => {}
            Err(error) => {
                state.error.get_or_insert(error);
                state.tasks.clear();
            }
        }
        if state.error.is_some() || (state.tasks.is_empty() && state.active == 0) {
            ready.notify_all();
        }
    }
}
fn run(task: Task, device: u64, report: Report) -> Result<Vec<Task>> {
    let fd = open_checked(task.parent.fd.as_raw_fd(), &task.name, device, task.inode)?;
    let node = Arc::new(Node {
        fd,
        parent: Some((task.parent, task.name)),
        pending: AtomicUsize::new(1),
    });
    let tasks = expand(&node, report)?;
    release(node)?;
    Ok(tasks)
}
/// Unlinks `node`'s files and makes a task of each subdirectory.
fn expand(node: &Arc<Node>, report: Report) -> Result<Vec<Task>> {
    let directories = sweep(node.fd.as_fd(), false, report)?;
    node.pending.fetch_add(directories.len(), Ordering::Relaxed);
    Ok(directories
        .into_iter()
        .map(|(name, inode)| Task {
            parent: node.clone(),
            name,
            inode,
        })
        .collect())
}
/// Drops one of `node`'s pending counts; at the last, removes it from its
/// parent and releases the parent in turn. The root is left to the caller.
fn release(mut node: Arc<Node>) -> Result<()> {
    loop {
        if node.pending.fetch_sub(1, Ordering::AcqRel) != 1 {
            return Ok(());
        }
        let parent = {
            let Some((parent, name)) = &node.parent else {
                return Ok(());
            };
            unlink(parent.fd.as_fd(), name, true)?;
            parent.clone()
        };
        node = parent;
    }
}

/// Lets the workers hold a deep tree's directories open: a GUI app on macOS
/// starts with 256 descriptors.
fn raise_open_file_limit() {
    static RAISED: Once = Once::new();
    RAISED.call_once(|| {
        // SAFETY: `limit` is plain data the calls read and fill.
        unsafe {
            let mut limit = std::mem::zeroed::<libc::rlimit>();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 && limit.rlim_cur < 4096 {
                limit.rlim_cur = limit.rlim_max.min(4096);
                libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
            }
        }
    });
}

/// Threads for a removal on `path`'s volume: several on solid-state
/// storage, one where it is rotational or cannot be told.
pub(super) fn workers_for(path: &str) -> usize {
    if super::media::solid_state(std::path::Path::new(path)) {
        std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_come_in_inode_order_with_their_own_inodes() {
        use std::os::unix::ffi::OsStrExt;
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".nsh/test-data/disktree-order-{}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("sub")).unwrap();
        for i in 0..200 {
            fs::write(dir.join(format!("file-{i}")), b"x").unwrap();
        }
        let fd = fs::File::open(&dir).unwrap();
        let entries = ordered(fd.as_fd()).unwrap();
        let inodes: Vec<u64> = entries
            .iter()
            .map(|entry| {
                let name = std::ffi::OsStr::from_bytes(entry.name.as_bytes());
                fs::symlink_metadata(dir.join(name)).unwrap().ino()
            })
            .collect();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(entries.len(), 201);
        assert!(entries.windows(2).all(|w| w[0].inode <= w[1].inode));
        assert!(entries.iter().zip(&inodes).all(|(e, ino)| e.inode == *ino));
    }
}
