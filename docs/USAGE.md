# Disktree for nsh

Development on macOS or Linux (nsh 0.1.1 and Rust; no JS runtime):

```
nsh run
```

`nsh dev` and `nsh run` open the current system’s native window with hot
reload. Mobile hosts require an explicit `--target ios` or `--target android`.

Filesystem scanning and cleanup run on background Rust threads inside the
native host. The interface, state, and treemap layout also use Rust; there is no JS bundle, backend process, or local HTTP server.
Release applications include
the scanner and do not require Python, Rust, Node or nsh to be installed.
For a different initial folder:
  DISKTREE_ROOT=~/Downloads nsh run

The app scans on launch. Use the pencil icon to edit a path and the refresh
icon to scan it. Click any breadcrumb to navigate; long paths fold their middle segments into the … menu. Click a tile or a contents row to
select it; Open folder or Enter descends, Parent or Option+Up ascends.
Large scan indexes stay in Rust. The interface fetches at most 80
items per directory page and a bounded treemap preview. Previous/Next
reaches every item; filtering searches the entire directory, not only
the displayed page. Aggregate tiles retain the size of items on other
pages and cannot be marked for deletion.

Use the breadcrumb trail to return to a scanned ancestor. Arrow keys select
siblings. Space marks a selection. Review marked items shows full paths;
Confirm move to Trash uses the system Trash. Delete permanently removes the
reviewed items after confirming their paths, reporting each file removed.
View options → Empty Trash clears
the system Trash after a separate confirmation.
Permanent removal cannot be undone. The map refreshes after removal.

Disk size measures allocated blocks, apparent size measures logical bytes,
and Files weights the map by file count. View options (☷) → 1–3 levels deep sets the map depth.
View options → Show hidden files toggles dot-prefixed direct children; Filter matches direct names across all pages.
Scan totals continue to describe the complete scan regardless of filters.

Symbolic links and other devices are not traversed. Hard-linked file bytes
are counted once; APFS clones are counted independently. Cloud-only
folders are skipped. Unreadable paths are reported, so totals may be partial.
Free space uses the operating system's disk-usage figure, excluding Finder's
additional purgeable-space estimate. Moving files to Trash does not free
space until Trash is emptied. Protected folders may require Full Disk Access
for the terminal running the scanner.

Checks:
  cargo test --locked
  cargo test --lib --release --locked -- --ignored --test-threads=1
  cargo clippy --all-targets --locked
  python3 -B -m unittest discover -s tests -p 'test_*.py' -v

Source: src/ui.rs (Rust components), src/state.rs (state and
background tasks), src/model.rs (classification and layout), and
src/scanner.rs (scanner and Trash).
Treemap transitions and pinch/pan run natively. Scan progress occupies the left
side of the fixed StatusBar. Metric changes do not change the Refresh button's
appearance. The sidebar is a navigation stack: Contents, then the selected
item's Details, then Review; Back or Esc pops a page. Right-click a tile or row
for its actions. Review appears only when items are marked; permanent removal
lives inside Review, and Empty Trash lives in View options. Removal progress
shows in Review and the StatusBar. Respects Reduce Motion.
Inspired by https://github.com/tobi/disktree.

The last successfully scanned or browsed directory is saved in
~/Library/Application Support/Disktree/state.json. On the next launch it
is restored and scanned automatically. A missing directory falls back
to the home directory. DISKTREE_ROOT overrides this startup location.
