//! Application state. Filesystem work never runs on the UI thread.
use crate::{model::*, scanner};
use nsh_core::ui::prelude::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Removal {
    Trash,
    Delete,
    Empty,
}
#[derive(Clone, Copy)]
pub(crate) struct State {
    pub owner: Owner,
    pub snapshot: Signal<Snapshot>,
    pub listing: Signal<Directory>,
    pub selection: Signal<Node>,
    pub path: Signal<String>,
    pub draft: Signal<String>,
    pub query: Signal<String>,
    pub metric: Signal<Metric>,
    pub listing_metric: Signal<Metric>,
    pub hidden: Signal<bool>,
    pub levels: Signal<u8>,
    pub page: Signal<usize>,
    pub busy: Signal<bool>,
    pub browsing: Signal<bool>,
    pub message: Signal<String>,
    pub progress: Signal<Option<Progress>>,
    pub seconds: Signal<u64>,
    pub marked: Signal<Vec<Node>>,
    pub reviewed: Signal<Vec<Node>>,
    pub review: Signal<bool>,
    pub removal: Signal<Removal>,
    pub editing: Signal<bool>,
    pub options: Signal<bool>,
    pub hovered: Signal<Vec<Node>>,
    pub in_map: Signal<bool>,
    pub generation: Signal<u64>,
    pub scan_generation: Signal<u64>,
    pub loaded: Signal<Value>,
    pub zoom: Signal<f32>,
}
async fn request(route: &'static str, data: Value) -> Result<Value, String> {
    blocking(move || scanner::dispatch(route, &data))
        .await
        .map_err(|e| e.to_string())?
}
impl State {
    pub fn new() -> Self {
        Self {
            owner: Owner::current(),
            snapshot: signal(Snapshot::default()),
            listing: signal(Directory::default()),
            selection: signal(Node::default()),
            path: signal(String::new()),
            draft: signal(String::new()),
            query: signal(String::new()),
            metric: signal(Metric::Size),
            listing_metric: signal(Metric::Size),
            hidden: signal(true),
            levels: signal(2),
            page: signal(0),
            busy: signal(false),
            browsing: signal(false),
            message: signal("Choose a folder to discover what fills your disk.".into()),
            progress: signal(None),
            seconds: signal(0),
            marked: signal(Vec::new()),
            reviewed: signal(Vec::new()),
            review: signal(false),
            removal: signal(Removal::Trash),
            editing: signal(false),
            options: signal(false),
            hovered: signal(Vec::new()),
            in_map: signal(false),
            generation: signal(0),
            scan_generation: signal(0),
            loaded: signal(Value::Null),
            zoom: signal(1.0),
        }
    }
    pub fn locked(self) -> bool {
        self.busy.get() || self.browsing.get()
    }
    pub fn parameters(self) -> Value {
        json!({"id":self.snapshot.get().id,"path":self.path.get(),"offset":self.page.get(),"query":self.query.get(),"hidden":self.hidden.get(),"metric":self.metric.get(),"levels":self.levels.get().max(if self.zoom.get()>=1.8 {3} else {1}),"fallback":true})
    }
    pub fn watch(self) {
        effect(move || {
            let args = self.parameters();
            if self.busy.get()
                || args["id"] == ""
                || args["path"] == ""
                || args == self.loaded.get_untracked()
            {
                return;
            }
            let run = self.generation.get_untracked() + 1;
            self.generation.set(run);
            self.browsing.set(true);
            self.owner.run(move || {
                spawn(async move {
                    let result = request("/view", args.clone()).await.and_then(|v| {
                        serde_json::from_value::<Directory>(v).map_err(|e| e.to_string())
                    });
                    if self.generation.get_untracked() != run {
                        return;
                    }
                    match result {
                        Ok(view) => {
                            let mut loaded = args.clone();
                            loaded["path"] = view.node.path.clone().into();
                            loaded["offset"] = view.offset.into();
                            self.loaded.set(loaded);
                            batch(|| {
                                self.listing_metric.set(self.metric.get_untracked());
                                let previous = self.listing.get_untracked().node.path;
                                if previous != view.node.path
                                    || self.selection.get_untracked().path == previous
                                {
                                    self.selection.set(view.node.clone());
                                }
                                self.path.set(view.node.path.clone());
                                self.page.set(view.offset);
                                self.listing.set(view);
                                self.browsing.set(false);
                            });
                        }
                        Err(e) => {
                            self.message.set(e);
                            self.browsing.set(false);
                        }
                    }
                })
            });
        });
    }
    pub fn scan(self, target: String) {
        if self.busy.get_untracked() {
            return;
        }
        let run = self.scan_generation.get_untracked() + 1;
        let token = format!("rust-{run}");
        batch(|| {
            self.scan_generation.set(run);
            self.generation.update(|n| *n += 1);
            self.busy.set(true);
            self.browsing.set(false);
            self.seconds.set(0);
            self.message.set("Scanning…".into());
            self.progress.set(Some(Progress {
                path: target.clone(),
                ..Default::default()
            }));
        });
        let poll_token = token.clone();
        self.owner.run(move || {
            spawn(async move {
                let started = Instant::now();
                while self.busy.get_untracked() && self.scan_generation.get_untracked() == run {
                    timer(Duration::from_millis(200)).await;
                    let value =
                        request("/scan-progress", json!({"progressToken":poll_token})).await;
                    if self.scan_generation.get_untracked() != run || !self.busy.get_untracked() {
                        break;
                    }
                    batch(|| {
                        self.seconds.set(started.elapsed().as_secs());
                        if let Ok(value) = value
                            && let Ok(progress) = serde_json::from_value::<Progress>(value)
                        {
                            self.progress.set(Some(progress));
                        }
                    });
                }
            })
        });
        self.owner.run(move || {
            spawn(async move {
                let result = request("/scan", json!({"path":target,"progressToken":token}))
                    .await
                    .and_then(|v| serde_json::from_value::<Snapshot>(v).map_err(|e| e.to_string()));
                if self.scan_generation.get_untracked() != run {
                    return;
                }
                batch(|| {
                    match result {
                        Ok(data) => {
                            self.path.set(data.root.path.clone());
                            self.selection.set(data.root.clone());
                            self.message.set(if data.error_count > 0 {
                                format!("{} unreadable or skipped paths.", data.error_count)
                            } else {
                                "Scan complete · symbolic links and other volumes are not followed."
                                    .into()
                            });
                            self.snapshot.set(data);
                            self.marked.set(Vec::new());
                            self.review.set(false);
                            self.query.set(String::new());
                            self.page.set(0);
                            self.loaded.set(Value::Null);
                            self.zoom.set(1.0);
                        }
                        Err(error) => self.message.set(error),
                    }
                    self.busy.set(false);
                    self.progress.set(None);
                });
            })
        });
    }
    pub fn enter(self, node: Node) {
        if self.locked() {
            return;
        }
        if node.aggregate {
            self.message
                .set("Use the directory list and page controls to explore remaining items.".into());
            return;
        }
        self.selection.set(node.clone());
        if node.directory {
            batch(|| {
                self.path.set(node.path);
                self.page.set(0);
                self.query.set(String::new());
                self.zoom.set(1.0);
                self.hovered.set(Vec::new());
            });
        }
    }
    pub fn choose(self, node: Node) {
        if node.aggregate || self.selection.get_untracked().path == node.path && node.directory {
            self.enter(node);
        } else {
            self.selection.set(node);
        }
    }
    pub fn navigate(self, path: String) {
        if self.locked() {
            return;
        }
        if let Some(node) = self
            .listing
            .get_untracked()
            .ancestors
            .into_iter()
            .find(|n| n.path == path)
        {
            self.enter(node);
        } else {
            self.scan(path);
        }
    }
    pub fn up(self) {
        let path = self.listing.get_untracked().node.path;
        if path.is_empty() || path == "/" {
            return;
        }
        self.navigate(
            path.rsplit_once('/')
                .map(|(p, _)| if p.is_empty() { "/" } else { p })
                .unwrap_or("/")
                .into(),
        );
    }
    pub fn mark(self, node: Node) {
        if self.locked()
            || node.aggregate
            || node.path.is_empty()
            || node.path == self.snapshot.get_untracked().root.path
        {
            return;
        }
        let mut marked = self.marked.get_untracked();
        if marked.iter().any(|n| n.path == node.path) {
            marked.retain(|n| n.path != node.path);
        } else {
            if marked
                .iter()
                .any(|n| node.path.starts_with(&(n.path.clone() + "/")))
            {
                self.message
                    .set("A parent folder is already marked.".into());
                return;
            }
            marked.retain(|n| !n.path.starts_with(&(node.path.clone() + "/")));
            marked.push(node);
        }
        self.marked.set(marked);
    }
    pub fn hover(self, node: Node, inside: bool) {
        self.hovered.update(|nodes| {
            nodes.retain(|n| n.path != node.path);
            if inside {
                nodes.push(node);
            }
        });
    }
    pub fn mark_pointer(self) {
        let node = self
            .hovered
            .get_untracked()
            .into_iter()
            .max_by_key(|n| n.path.len());
        if let Some(node) =
            node.or_else(|| (!self.in_map.get_untracked()).then(|| self.selection.get_untracked()))
        {
            self.mark(node);
        }
    }
    pub fn begin_review(self, mode: Removal) {
        if self.locked() || mode != Removal::Empty && self.marked.get_untracked().is_empty() {
            return;
        }
        batch(|| {
            self.reviewed.set(self.marked.get_untracked());
            self.removal.set(mode);
            self.review.set(true);
        });
    }
    pub fn commit(self) {
        if self.locked()
            || self.removal.get_untracked() != Removal::Empty
                && self.reviewed.get_untracked().is_empty()
        {
            return;
        }
        self.busy.set(true);
        self.generation.update(|n| *n += 1);
        self.owner.run(move || spawn(async move {
            let mode=self.removal.get_untracked();
            let route=match mode{Removal::Trash=>"/trash",Removal::Delete=>"/delete",Removal::Empty=>"/empty-trash"};
            let result=request(route,json!({"id":self.snapshot.get_untracked().id,"paths":self.reviewed.get_untracked().iter().map(|n|n.path.clone()).collect::<Vec<_>>()})).await;
            batch(||{
                match result {
                    Ok(result)=>{
                        let completed:Vec<String>=serde_json::from_value(result["completed"].clone()).unwrap_or_default();
                        let retain=|n:&Node|!completed.iter().any(|p|n.path==*p || n.path.starts_with(&(p.clone()+"/")));
                        self.marked.update(|nodes|nodes.retain(retain));self.reviewed.update(|nodes|nodes.retain(retain));
                        if let Some(patch)=result.get("snapshot").and_then(Value::as_object){
                            self.snapshot.update(|data|{
                                if let Some(id)=patch.get("id").and_then(Value::as_str){data.id=id.into();}
                                if let Some(root)=patch.get("root").and_then(|v|serde_json::from_value(v.clone()).ok()){data.root=root;}
                                if let Some(free)=patch.get("free").and_then(Value::as_u64){data.free=free;}
                            });
                        }
                        let error=result.get("error").and_then(Value::as_str);
                        self.message.set(error.map(str::to_owned).unwrap_or_else(||if mode==Removal::Empty{"Trash emptied.".into()}else{format!("{} items removed.",completed.len())}));
                        self.review.set(error.is_some());self.loaded.set(Value::Null);
                    }
                    Err(e)=>self.message.set(e),
                }
                self.busy.set(false);
            });
        }));
    }
}
