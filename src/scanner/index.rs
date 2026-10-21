//! Disposable disk index. Page caches and query results are bounded independently
//! of the number of files; no complete node, name, or path table lives in RAM.
use super::*;
use rusqlite::{Connection, OptionalExtension, Row, functions::FunctionFlags, params};
use std::sync::MutexGuard;

pub(super) type NodeId = u32;
pub(super) const NO_PARENT: NodeId = NodeId::MAX;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Node {
    pub record: Record,
    pub name: String,
    pub size: u64,
    pub apparent: u64,
    pub files: u64,
    pub parent: NodeId,
    pub directory: bool,
    pub linked: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Totals {
    pub count: usize,
    pub size: u64,
    pub apparent: u64,
    pub files: u64,
}
impl Totals {
    pub fn add(&mut self, node: &Node) {
        self.count += 1;
        self.size += node.size;
        self.apparent += node.apparent;
        self.files += node.files;
    }
    pub fn minus(self, other: Self) -> Self {
        Self {
            count: self.count - other.count,
            size: self.size - other.size,
            apparent: self.apparent - other.apparent,
            files: self.files - other.files,
        }
    }
}
// Clones refer to the same scan. The connection mutex serializes edits with
// directory reads, without cloning the full index during removal.
#[derive(Clone)]
pub(super) struct Index {
    pub id: String,
    pub root_path: String,
    pub live: usize,
    pub access: Arc<Mutex<()>>,
    store: Arc<Mutex<Connection>>,
}

pub(super) fn temporary_database(cache_kib: usize) -> Result<Connection> {
    // An empty filename creates a private on-disk temporary database; SQLite
    // removes it on close, including error paths. Disable mmap so a large scan
    // cannot turn the entire database into resident process pages.
    let db = Connection::open("").map_err(io)?;
    db.execute_batch(&format!(
        "PRAGMA page_size=4096;
        PRAGMA cache_size=-{cache_kib}; PRAGMA temp_store=FILE;
        PRAGMA mmap_size=0; PRAGMA cache_spill=ON;
        PRAGMA synchronous=OFF; PRAGMA journal_mode=DELETE;
        PRAGMA auto_vacuum=FULL;"
    ))
    .map_err(io)?;
    Ok(db)
}
fn read_node(row: &Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        parent: row.get(0)?,
        name: row.get(1)?,
        directory: row.get(2)?,
        linked: row.get(3)?,
        record: Record {
            device: row.get::<_, i64>(4)? as u64,
            inode: row.get::<_, i64>(5)? as u64,
            modified: row.get(6)?,
            nanos: row.get(7)?,
            size: row.get::<_, i64>(8)? as u64,
        },
        size: row.get::<_, i64>(9)? as u64,
        apparent: row.get::<_, i64>(10)? as u64,
        files: row.get::<_, i64>(11)? as u64,
    })
}
const NODE_COLUMNS: &str =
    "parent,name,directory,linked,device,inode,modified,nanos,logical,size,apparent,files";
impl Index {
    pub fn new(root: &str) -> Result<Self> {
        let db = temporary_database(8192)?;
        db.create_scalar_function(
            "contains_fold",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                Ok(ctx
                    .get::<String>(0)?
                    .to_lowercase()
                    .contains(&ctx.get::<String>(1)?))
            },
        )
        .map_err(io)?;
        db.execute_batch(
            "CREATE TABLE nodes (
            id INTEGER PRIMARY KEY, parent INTEGER NOT NULL, name TEXT NOT NULL,
            directory INTEGER NOT NULL, linked INTEGER NOT NULL,
            device INTEGER NOT NULL, inode INTEGER NOT NULL,
            modified INTEGER NOT NULL, nanos INTEGER NOT NULL, logical INTEGER NOT NULL,
            size INTEGER NOT NULL, apparent INTEGER NOT NULL, files INTEGER NOT NULL);
            CREATE UNIQUE INDEX child_names ON nodes(parent,name);
            CREATE INDEX hard_links ON nodes(device,inode) WHERE linked=1;
            BEGIN;",
        )
        .map_err(io)?;
        Ok(Self {
            id: String::new(),
            root_path: root.to_owned(),
            live: 0,
            access: Arc::new(Mutex::new(())),
            store: Arc::new(Mutex::new(db)),
        })
    }
    pub fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.store.lock().map_err(io)
    }
    pub fn insert(&mut self, node: Node) -> Result<NodeId> {
        let id = NodeId::try_from(self.live).map_err(|_| "Too many scan entries")?;
        if id == NO_PARENT {
            return Err("Too many scan entries".into());
        }
        self.db()?
            .prepare_cached("INSERT INTO nodes VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)")
            .map_err(io)?
            .execute(params![
                id,
                node.parent,
                node.name,
                node.directory,
                node.linked,
                node.record.device as i64,
                node.record.inode as i64,
                node.record.modified,
                node.record.nanos,
                node.record.size as i64,
                node.size as i64,
                node.apparent as i64,
                node.files as i64
            ])
            .map_err(io)?;
        self.live += 1;
        Ok(id)
    }
    pub fn node_optional(&self, id: NodeId) -> Result<Option<Node>> {
        self.db()?
            .prepare_cached(&format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id=?1"))
            .map_err(io)?
            .query_row([id], read_node)
            .optional()
            .map_err(io)
    }
    pub fn node(&self, id: NodeId) -> Result<Node> {
        self.node_optional(id)?
            .ok_or_else(|| "Scan entry no longer exists".into())
    }
    pub fn root(&self) -> Result<Node> {
        self.node(0)
    }
    pub fn path(&self, mut id: NodeId) -> Result<String> {
        let mut parts = Vec::new();
        while id != 0 {
            let node = self.node(id)?;
            parts.push(node.name);
            id = node.parent;
        }
        let mut path = self.root_path.clone();
        for part in parts.into_iter().rev() {
            if !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(&part);
        }
        Ok(path)
    }
    pub fn child_named(&self, parent: NodeId, name: &str) -> Result<Option<NodeId>> {
        self.db()?
            .prepare_cached("SELECT id FROM nodes WHERE parent=?1 AND name=?2")
            .map_err(io)?
            .query_row(params![parent, name], |row| row.get(0))
            .optional()
            .map_err(io)
    }
    pub fn lookup(&self, path: &str) -> Result<Option<NodeId>> {
        let Ok(relative) = Path::new(path).strip_prefix(&self.root_path) else {
            return Ok(None);
        };
        let mut id = 0;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Ok(None);
            };
            let Some(name) = name.to_str() else {
                return Ok(None);
            };
            let Some(child) = self.child_named(id, name)? else {
                return Ok(None);
            };
            id = child;
        }
        Ok(Some(id))
    }
    pub fn children(&self, id: NodeId) -> Result<Vec<NodeId>> {
        self.db()?
            .prepare_cached("SELECT id FROM nodes WHERE parent=?1 ORDER BY name")
            .map_err(io)?
            .query_map([id], |row| row.get(0))
            .map_err(io)?
            .collect::<rusqlite::Result<_>>()
            .map_err(io)
    }
    pub fn shallow(&self, id: NodeId) -> Result<Value> {
        let node = self.node(id)?;
        Ok(
            json!({"path":self.path(id)?,"name":node.name,"directory":node.directory,"size":node.size,
            "apparent":node.apparent,"files":node.files,
            "modified":node.record.modified as f64 + node.record.nanos as f64 / 1e9,"children":[]}),
        )
    }
    pub fn weights(&self, ids: &[NodeId]) -> Result<Totals> {
        let mut total = Totals::default();
        for id in ids {
            total.add(&self.node(*id)?);
        }
        Ok(total)
    }
    // Sorting spills to disk. Only the requested page (or bounded preview) is
    // returned even for a directory containing millions of immediate children.
    pub fn listing(
        &self,
        parent: NodeId,
        hidden: bool,
        query: &str,
        metric: &str,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<NodeId>, Totals)> {
        let column = match metric {
            "size" => "size",
            "files" => "files",
            "apparent" => "apparent",
            _ => return Err("Invalid metric".into()),
        };
        let filter =
            "parent=?1 AND (?2 OR substr(name,1,1)!='.') AND (?3='' OR contains_fold(name,?3))";
        let db = self.db()?;
        let total = db.prepare_cached(&format!("SELECT count(*),coalesce(sum(size),0),coalesce(sum(apparent),0),coalesce(sum(files),0) FROM nodes WHERE {filter}"))
            .map_err(io)?.query_row(params![parent,hidden,query], |row| Ok(Totals { count: row.get::<_,i64>(0)? as usize,
                size: row.get::<_,i64>(1)? as u64, apparent: row.get::<_,i64>(2)? as u64, files: row.get::<_,i64>(3)? as u64 })).map_err(io)?;
        let offset = offset.min(total.count.saturating_sub(1) / PAGE_SIZE * PAGE_SIZE);
        let ids = db
            .prepare_cached(&format!(
                "SELECT id FROM nodes WHERE {filter} ORDER BY {column} DESC,name LIMIT ?4 OFFSET ?5"
            ))
            .map_err(io)?
            .query_map(
                params![parent, hidden, query, limit as i64, offset as i64],
                |row| row.get(0),
            )
            .map_err(io)?
            .collect::<rusqlite::Result<_>>()
            .map_err(io)?;
        Ok((ids, total))
    }
    pub fn reduce(&self, id: NodeId) -> Result<()> {
        self.db()?.prepare_cached("UPDATE nodes SET (size,apparent,files)=(SELECT coalesce(sum(size),0),coalesce(sum(apparent),0),coalesce(sum(files),0) FROM nodes WHERE parent=?1) WHERE id=?1")
            .map_err(io)?.execute([id]).map_err(io)?;
        Ok(())
    }
    pub fn finish(&self) -> Result<()> {
        let mut before = NO_PARENT;
        loop {
            let ids: Vec<NodeId> = self
                .db()?
                .prepare_cached(
                    "SELECT id FROM nodes WHERE directory=1 AND id<?1 ORDER BY id DESC LIMIT 256",
                )
                .map_err(io)?
                .query_map([before], |row| row.get(0))
                .map_err(io)?
                .collect::<rusqlite::Result<_>>()
                .map_err(io)?;
            if ids.is_empty() {
                break;
            }
            before = *ids.last().unwrap();
            for id in ids {
                self.reduce(id)?;
            }
        }
        self.db()?
            .execute_batch("COMMIT; PRAGMA shrink_memory;")
            .map_err(io)
    }
    pub fn find_link(&self, device: u64, inode: u64) -> Result<Option<NodeId>> {
        self.db()?
            .prepare_cached(
                "SELECT id FROM nodes WHERE linked=1 AND device=?1 AND inode=?2 LIMIT 1",
            )
            .map_err(io)?
            .query_row(params![device as i64, inode as i64], |row| row.get(0))
            .optional()
            .map_err(io)
    }
    pub fn set_weights(&self, id: NodeId, size: u64, apparent: u64) -> Result<()> {
        self.db()?
            .prepare_cached("UPDATE nodes SET size=?2,apparent=?3 WHERE id=?1")
            .map_err(io)?
            .execute(params![id, size as i64, apparent as i64])
            .map_err(io)?;
        Ok(())
    }
    pub fn set_record(&self, id: NodeId, record: &Record) -> Result<()> {
        self.db()?
            .prepare_cached(
                "UPDATE nodes SET device=?2,inode=?3,modified=?4,nanos=?5,logical=?6 WHERE id=?1",
            )
            .map_err(io)?
            .execute(params![
                id,
                record.device as i64,
                record.inode as i64,
                record.modified,
                record.nanos,
                record.size as i64
            ])
            .map_err(io)?;
        Ok(())
    }
    pub fn delete(&mut self, id: NodeId) -> Result<()> {
        let removed = self
            .db()?
            .prepare_cached("DELETE FROM nodes WHERE id=?1")
            .map_err(io)?
            .execute([id])
            .map_err(io)?;
        self.live -= removed;
        Ok(())
    }
    pub fn trash_directories(&self, uid: &str) -> Result<Vec<NodeId>> {
        self.db()?
            .prepare_cached("SELECT id FROM nodes WHERE directory=1 AND name=?1")
            .map_err(io)?
            .query_map([uid], |row| row.get(0))
            .map_err(io)?
            .collect::<rusqlite::Result<_>>()
            .map_err(io)
    }
    #[cfg(test)]
    pub fn all_ids(&self) -> Result<Vec<NodeId>> {
        self.db()?
            .prepare("SELECT id FROM nodes ORDER BY id")
            .map_err(io)?
            .query_map([], |row| row.get(0))
            .map_err(io)?
            .collect::<rusqlite::Result<_>>()
            .map_err(io)
    }
}
