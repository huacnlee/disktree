//! Bounded filesystem views and the treemap's platform-independent geometry.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Node {
    pub path: String,
    pub name: String,
    pub directory: bool,
    pub aggregate: bool,
    pub size: u64,
    pub apparent: u64,
    pub files: u64,
    pub modified: f64,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Metric {
    #[default]
    Size,
    Apparent,
    Files,
}
impl Metric {
    pub fn weight(self, node: &Node) -> u64 {
        match self {
            Self::Size => node.size,
            Self::Apparent => node.apparent,
            Self::Files => node.files,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Size => "Disk size",
            Self::Apparent => "Apparent",
            Self::Files => "Files",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub id: String,
    pub root: Node,
    pub total: u64,
    pub free: u64,
    pub errors: Vec<String>,
    pub error_count: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Directory {
    pub node: Node,
    pub children: Vec<Node>,
    pub map: Vec<Node>,
    pub ancestors: Vec<Node>,
    pub offset: usize,
    pub count: usize,
    pub page_size: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(crate) struct Progress {
    pub path: String,
    pub directories: u64,
    pub files: u64,
    pub bytes: u64,
    pub errors: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tile {
    pub node: Node,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: u8,
}

pub(crate) const CATEGORIES: [(&str, u32); 10] = [
    ("Code", 0x477de0),
    ("Git", 0xe65883),
    ("Caches", 0xe9b64d),
    ("Builds", 0x43cda1),
    ("Toolchains", 0x9ecb4e),
    ("Agent data", 0xe38b51),
    ("Media", 0xaa6ee5),
    ("Documents", 0x72a4b6),
    ("Synced", 0x42b9c6),
    ("Other", 0x626d81),
];

pub(crate) fn category(path: &str) -> (&'static str, u32) {
    let path = path.to_lowercase();
    let parts: Vec<_> = path.split('/').collect();
    let has = |names: &[&str]| parts.iter().any(|p| names.contains(p));
    let extension = path.rsplit('.').next().unwrap_or("");
    let index = if has(&[".git"]) {
        1
    } else if has(&["cache", "caches", ".cache", ".caches", "node_modules"])
        || (path.contains("/pkg/mod/") || path.ends_with("/pkg/mod"))
    {
        2
    } else if has(&["target", "build", "dist", "deriveddata"])
        || parts.iter().any(|p| p.starts_with("target-"))
    {
        3
    } else if has(&[
        ".cargo",
        ".rustup",
        "toolchains",
        "coresimulator",
        "xcode",
        "android",
        ".android",
        ".bun",
        ".npm",
    ]) {
        4
    } else if has(&[".codex", ".claude", ".cursor", "sessions"]) {
        5
    } else if has(&["pictures", "movies", "music"])
        || [
            "mp4", "mov", "mp3", "wav", "jpg", "jpeg", "png", "heic", "webp", "gif",
        ]
        .contains(&extension)
    {
        6
    } else if has(&["dropbox", "onedrive", "icloud", "mobile documents"]) {
        8
    } else if has(&["documents"])
        || ["pdf", "doc", "docx", "txt", "md", "pages", "xls", "xlsx"].contains(&extension)
    {
        7
    } else if has(&["src", "code", "work", "github"])
        || ["ts", "tsx", "js", "jsx", "rs", "py", "go", "rb", "swift"].contains(&extension)
    {
        0
    } else {
        9
    };
    CATEGORIES[index]
}

pub(crate) fn tile_color(node: &Node, depth: u8) -> u32 {
    let base = category(&node.path).1;
    let hash = node
        .name
        .encode_utf16()
        .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
    let shift = (hash.unsigned_abs() % 19) as i32 - 9 + (depth as i32 - 1) * 5;
    [16, 8, 0].into_iter().fold(0, |color, bit| {
        color | (((((base >> bit) & 255) as i32 + shift).clamp(0, 255) as u32) << bit)
    })
}

pub(crate) fn format_size(mut value: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut at = 0;
    while value >= 1024.0 && at < units.len() - 1 {
        value /= 1024.0;
        at += 1;
    }
    if at == 0 {
        format!("{value:.0} {}", units[at])
    } else {
        format!("{value:.1} {}", units[at])
    }
}

pub(crate) fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

/// Balanced subdivision preserves area and bounds the work to the supplied view.
pub(crate) fn mosaic(
    nodes: &[Node],
    width: f32,
    height: f32,
    metric: Metric,
    levels: u8,
    zoom: f32,
) -> Vec<Tile> {
    fn ordered(nodes: &[Node], metric: Metric) -> Vec<&Node> {
        let mut nodes: Vec<_> = nodes.iter().filter(|n| metric.weight(n) > 0).collect();
        nodes.sort_by_key(|node| std::cmp::Reverse(metric.weight(node)));
        nodes
    }
    fn split(
        nodes: &[&Node],
        rect: [f32; 4],
        metric: Metric,
        levels: u8,
        zoom: f32,
        depth: u8,
        out: &mut Vec<Tile>,
    ) {
        let [x, y, width, height] = rect;
        if nodes.is_empty() || width * zoom < 2.0 || height * zoom < 2.0 {
            return;
        }
        if nodes.len() == 1 {
            let node = nodes[0];
            out.push(Tile {
                node: node.clone(),
                x,
                y,
                width,
                height,
                depth,
            });
            if depth < levels
                && !node.children.is_empty()
                && width * zoom > 90.0
                && height * zoom > 65.0
            {
                split(
                    &ordered(&node.children, metric),
                    [
                        x + 3.0 / zoom,
                        y + 25.0 / zoom,
                        width - 6.0 / zoom,
                        height - 28.0 / zoom,
                    ],
                    metric,
                    levels,
                    zoom,
                    depth + 1,
                    out,
                );
            }
            return;
        }
        let total: f64 = nodes.iter().map(|n| metric.weight(n) as f64).sum();
        let mut sum = 0.0;
        let mut at = 0;
        while at < nodes.len() - 1 && sum < total / 2.0 {
            sum += metric.weight(nodes[at]) as f64;
            at += 1;
        }
        let ratio = (sum / total) as f32;
        let (first, second) = if width >= height {
            (
                [x, y, width * ratio, height],
                [x + width * ratio, y, width * (1.0 - ratio), height],
            )
        } else {
            (
                [x, y, width, height * ratio],
                [x, y + height * ratio, width, height * (1.0 - ratio)],
            )
        };
        split(&nodes[..at], first, metric, levels, zoom, depth, out);
        split(&nodes[at..], second, metric, levels, zoom, depth, out);
    }
    let mut result = Vec::new();
    split(
        &ordered(nodes, metric),
        [0.0, 0.0, width, height],
        metric,
        levels,
        zoom,
        1,
        &mut result,
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specific_categories_precede_broad_ancestors() {
        for (path, expected) in [
            ("/Users/jason/work/project/.git/objects", "Git"),
            ("/Users/jason/work/project/target-gallery", "Builds"),
            ("/Users/jason/work/pkg/mod/cache", "Caches"),
            ("/Users/jason/work/pkg/mod", "Caches"),
            ("/Users/jason/work/project/photo.jpg", "Media"),
            ("/Users/jason/.rustup/toolchains", "Toolchains"),
            ("/Users/jason/Documents/letter.pdf", "Documents"),
        ] {
            assert_eq!(category(path).0, expected);
        }
    }
    #[test]
    fn subdivision_preserves_area_and_changes_metric() {
        let nodes = vec![
            Node {
                name: "a".into(),
                path: "/a".into(),
                size: 9,
                files: 1,
                ..Default::default()
            },
            Node {
                name: "b".into(),
                path: "/b".into(),
                size: 1,
                files: 9,
                ..Default::default()
            },
        ];
        for metric in [Metric::Size, Metric::Files] {
            let tiles = mosaic(&nodes, 100.0, 80.0, metric, 1, 1.0);
            assert_eq!(tiles.len(), 2);
            assert!((tiles.iter().map(|t| t.width * t.height).sum::<f32>() - 8000.0).abs() < 0.01);
            assert_eq!(
                tiles[0].node.name,
                if metric == Metric::Size { "a" } else { "b" }
            );
        }
    }
}
