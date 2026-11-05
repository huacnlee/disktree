# Disktree for nsh

Development on macOS or Linux (nsh 0.4.0 and Rust; no JS runtime):

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

## App updates

Installed builds check for updates in the background on startup. A new version
adds **Update available** to the status bar without opening a dialog. Open it,
download the update, then choose **Install and restart**. You can also check
manually through **View options → Check for updates…**. Restart stays disabled
while scanning or removing files. The saved directory stays outside the app
installation and is restored by the new process. A read-only installation offers
a manual download link instead.

Disktree is a native Rust app: `src/updater.rs` uses `nsh-updater` for manifest
signatures, version checks, archive verification and transactional installation,
and streams HTTP on a background thread. UI dependencies use NSH 0.4.0; the
updater is fixed to #922’s merged commit (`12af3d77`) until a crates.io release
includes it.

`scripts/build-release.py` retains the existing flat installer archive and also
writes a one-directory update archive and a platform manifest. The release
workflow merges the platform manifests into `latest.json` and validates the
archives before publishing. Automatic updates require that manifest in the next
GitHub Release; this integration does not publish a release.

To exercise actual packaged GUI applications with an ephemeral signing key and
a loopback server:

```sh
python3 scripts/smoke-updater.py
```

It builds test versions 0.1.6 and 0.1.7, tests download/replacement/restart and
state preservation, quiet automatic checks, and refuses modified signatures, archive size/hash changes,
a different app, non-executable replacements, read-only installs and Linux launch
failures. It only replaces disposable copies under `.nsh/test-data`, never the
user's installation. OpenSSL with Ed25519 support is required for this test.

The **Desktop checks** workflow runs Rust tests/lint, installer/manifest checks
and the real packaged GUI update scenarios on Linux and macOS. These checks do
not publish a release. The separate Release workflow publishes `latest.json`
alongside the normal installer and the longer `-updater.tar.gz` payload.
