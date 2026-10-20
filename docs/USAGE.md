# Disktree for nsh

Development on macOS or Linux (nsh 0.1.1 and Rust; Python 3 for setup tools only):

```
nsh run
```

`nsh dev` and `nsh run` open the current system’s native window with hot
reload. Mobile hosts require an explicit `--target ios` or `--target android`.

Filesystem scanning and cleanup run on background Rust threads inside the
native host. JSX calls the Rust module directly; there is no backend process
or local HTTP server.
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
selected item after reviewing its path; Delete marked permanently removes a
batch. Empty Trash clears the system Trash after a separate confirmation.
Permanent removal cannot be undone. The map refreshes after removal.

Disk size measures allocated blocks, apparent size measures logical bytes,
and Files weights the map by file count. Settings icon → Treemap depth selects 1–3 levels.
Settings icon → Show hidden files toggles dot-prefixed direct children; Filter matches direct names across all pages.
Scan totals continue to describe the complete scan regardless of filters.

Symbolic links and other devices are not traversed. Hard-linked file bytes
are counted once; APFS clones are counted independently. Cloud-only
folders are skipped. Unreadable paths are reported, so totals may be partial.
Free space uses the operating system's disk-usage figure, excluding Finder's
additional purgeable-space estimate. Moving files to Trash does not free
space until Trash is emptied. Protected folders may require Full Disk Access
for the terminal running the scanner.

Checks:
  nsh check
  nsh build
  cargo test --manifest-path native/Cargo.toml --lib --locked
  cargo test --manifest-path native/Cargo.toml --lib --locked -- --ignored
  cargo test --manifest-path native/Cargo.toml --test ui --locked -- --ignored
  python3 -m unittest discover -s tests -p 'test_*.py' -v

Source: src/App.tsx (JSX UI), src/disk.ts (classification and treemap layout),
native/src/lib.rs (JSX native module), native/src/scanner.rs (scanner and Trash).
Visual direction: a black interface with subtle dimensional surfaces, saturated category
colors, an original capacity-ring icon, and amber removal marks. Native
motion includes an entrance reveal, scan sweep, pinch/pan, and a review-panel
reveal. Tiles retain the original subtle category tints and diagonal gradient;
large maps avoid individual tile animations and blurred shadows. Respects Reduce Motion.
Inspired by https://github.com/tobi/disktree.

The last successfully scanned or browsed directory is saved in
~/Library/Application Support/Disktree/state.json. On the next launch it
is restored and scanned automatically. A missing directory falls back
to the home directory. DISKTREE_ROOT overrides this startup location.
