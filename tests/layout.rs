//! Visible content needs a laid-out size: a box of zero height renders nothing
//! even though its elements exist, which the interaction checks cannot see.
use nsh_core::{
    MountItem, NodeId,
    geometry::{px, size},
    ui::Root,
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Default)]
struct Tree {
    frames: HashMap<NodeId, (f32, f32)>,
    parents: HashMap<NodeId, NodeId>,
    texts: HashMap<NodeId, String>,
}
#[test]
fn sidebar_and_treemap_fill_their_space() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // The integration test is the sole test in its process; set before the engine starts.
    unsafe {
        std::env::set_var("DISKTREE_ROOT", format!("{manifest}/src"));
        std::env::set_var(
            "DISKTREE_STATE",
            format!("{manifest}/.nsh/test-data/layout-state.json"),
        );
    }
    let tree = Arc::new(Mutex::new(Tree::default()));
    let observed = tree.clone();
    let mut ui = Headless::start(EngineConfig {
        viewport: size(px(1280.), px(850.)),
        ..EngineConfig::new(Root::of::<disktree::App>())
    })
    .unwrap();
    ui.observe(move |_, batch| {
        let mut tree = observed.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Frame { id, frame } => {
                    tree.frames
                        .insert(*id, (frame.size.width.0, frame.size.height.0));
                }
                MountItem::Insert { parent, child, .. } => {
                    tree.parents.insert(*child, *parent);
                }
                MountItem::Text { id, text, .. } => {
                    tree.texts.insert(*id, text.text.to_string());
                }
                _ => {}
            }
        }
    });
    ui.wait_for("text:LARGEST").unwrap();
    ui.wait_for("label:Select lib.rs").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let tree = tree.lock().unwrap();
    // Each text's ancestors' (width, height), innermost first.
    let chains = |text: &str| -> Vec<Vec<(f32, f32)>> {
        tree.texts
            .iter()
            .filter(|(_, t)| *t == text)
            .map(|(id, _)| {
                let mut id = id;
                let mut frames = vec![];
                while let Some(parent) = tree.parents.get(id) {
                    frames.push(tree.frames.get(id).copied().unwrap_or_default());
                    id = parent;
                }
                frames
            })
            .collect()
    };
    for text in ["LARGEST", "DISK SPACE", "lib.rs"] {
        for chain in chains(text) {
            assert!(chain.iter().all(|f| f.1 > 0.), "{text}: {chain:?}");
        }
    }
    // The map (its boxes as wide as the left pane) takes the pane's height,
    // not the frame it starts with.
    let map = chains("lib.rs")
        .into_iter()
        .find(|chain| chain.iter().any(|f| f.0 > 900. && f.0 < 1000.))
        .expect("a map tile shows lib.rs");
    for frame in map.iter().filter(|f| f.0 > 900. && f.0 < 1000.) {
        assert!(frame.1 > 600., "map box {frame:?} in {map:?}");
    }
}
