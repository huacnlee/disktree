# Disktree

A native disk-space explorer built with **nsh**, inspired by [tobi/disktree](https://github.com/tobi/disktree). Runs on macOS and Linux with an original capacity-ring icon and a black interface with subtle dimensional surfaces and vivid category colors.

<img width="1303" height="893" alt="image" src="https://github.com/user-attachments/assets/c47b62b3-f03a-4d82-a33a-7197c254aaff" />

- Scans immediately on launch and remembers the last directory.
- Nested, color-coded treemap for code, Git, caches, builds, toolchains, media, and more.
- Native MacBook pinch gestures and mouse dragging; keyboard and button zoom controls.
- Click to select; click a selected folder again to enter it. Breadcrumbs navigate with one click.
- Point at a tile and press Space to toggle its removal mark.
- Amber fill, diagonal hatching, and check badges distinguish marked files and folders.
- Review full paths before moving items to the system Trash. Permanent deletion and emptying the system Trash require separate confirmation.
- Large scan indexes stay in Rust; the UI reads bounded directory pages and map previews.

## Install

Release archives include the native application with an in-process Rust scanner. Python, Node, and nsh are not required to run them.

### mise

```sh
mise use -g github:huacnlee/nsh-disktree
# Install without selecting a global version:
# mise install github:huacnlee/nsh-disktree@latest
disktree
```

The [mise GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html) selects the release asset for your platform.

### curl

```sh
curl -fsSL https://raw.githubusercontent.com/huacnlee/nsh-disktree/main/install.sh | sh
```

To install a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/huacnlee/nsh-disktree/main/install.sh | sh -s -- v0.1.0
```

The installer verifies the release's SHA-256 checksum. It installs the command in `~/.local/bin/disktree`, with a macOS app entry in `~/Applications`. Override these locations with `DISKTREE_BIN_DIR`, `DISKTREE_INSTALL_DIR`, and `DISKTREE_APPS_DIR`. Run the installer again to upgrade.

### Manual download

Download the matching `.tar.gz` from [Releases](https://github.com/huacnlee/nsh-disktree/releases), extract the complete archive, and run `./disktree`. On macOS, you can also open `Disktree.app`. Keep its accompanying resources together.

macOS requires 14 or later. Builds use ad hoc signing and are not notarized; first launch may require allowing the app in System Settings → Privacy & Security. Linux requires GTK 4.14 or later, such as Ubuntu 24.04:

```sh
sudo apt install libgtk-4-1 libglib2.0-bin
```

## Controls

| Action | Control |
| --- | --- |
| Scan a folder | Click ↻; use ✎ to edit a path |
| Select a tile | Click, or use the arrow keys |
| Enter a folder | Click the selected folder again, Enter, or Open folder |
| Navigate back | Click a breadcrumb, or Alt+↑ |
| Zoom | Pinch out/in, use +/− buttons, or Cmd/Ctrl + / - |
| Pan | Drag the map with the mouse |
| Fit the map | Click the percentage or press Cmd/Ctrl+0 |
| Mark/unmark | Point at a tile and press Space, or use Mark / unmark |
| Review removal | Review marked items → inspect paths → Confirm move to Trash |
| Delete permanently | Select an item → Delete permanently → inspect path → Confirm permanent deletion |
| Empty Trash | Empty Trash → Confirm empty Trash |

Marking does not delete anything. Successful removal automatically refreshes the map and free space. Use Empty Trash to reclaim space, or Delete marked permanently to bypass Trash; both require confirmation.

Directory lists contain 80 items per page. Filtering searches all direct children of the current directory, including other pages. Aggregate tiles preserve the weight of undisplayed items and cannot be marked. Enter individual folders to inspect their contents.

The last directory is saved in `~/Library/Application Support/Disktree/state.json` on macOS and `${XDG_STATE_HOME:-~/.local/state}/nsh-disktree/state.json` on Linux. `DISKTREE_ROOT` overrides the initial directory; `DISKTREE_STATE` overrides the state file.

## Project layout

- `App.tsx`: the thin nsh entry point.
- `src/`: JSX views, treemap layout, and the native connection adapter.
- `native/`: Rust native module and packaged desktop host.
- `scripts/`: development setup and release packaging tools only.
- `tests/`: JSX regression fixtures and installer tests. Rust tests live in `native/`.
- `docs/`: detailed [usage notes](docs/USAGE.md).
- `assets/`: application icon.

## Development

The project uses nsh 0.1.1's **Rust + JSX** mode. `src/` owns the interface; `native/src/lib.rs` exports `disktree/native`, and `native/src/scanner.rs` implements scanning, bounded directory queries, persistence, and cleanup. JSX calls `Scanner.request()` directly. Filesystem work runs on Rust background threads in the same process: no Python service, loopback HTTP server, generated connection file, or hover patch.

Install [nsh 0.1.1](https://github.com/longbridge/nsh/releases/tag/v0.1.1), Rust and the platform build libraries listed below. Python 3 is used only for the setup/packaging convenience tools, never for application runtime. nsh 0.1.1's custom Rust hosts require an upstream source checkout (`NSH_SOURCE`); prepare the pinned, unmodified release once:

```sh
python3 scripts/nsh_toolchain.py
export NSH_SOURCE="$PWD/.runtime-source"
nsh run
```

After setting `NSH_SOURCE` in the development shell, `nsh run` starts the native host with its in-process scanner. No separate backend process is started. `./scripts/run.sh` remains a convenience wrapper for preparation and environment setup; it accepts the same arguments as `nsh run`.

JSX edits hot reload; changes to the Rust module rebuild the custom host. The first run compiles the app's CLI and host. Cargo artifacts stay in `.build/cargo/`. Rust capabilities use published `nsh-core` / platform crates pinned to `=0.1.1`; `native/Cargo.lock` locks their dependencies.

Build a self-contained application with Rust, Python 3.11+ (packaging tools only), and Xcode Command Line Tools on macOS, or `clang libclang-dev libgtk-4-dev` on Linux:

```sh
python3 scripts/build-release.py --version 0.1.1
```

The first build downloads the pinned upstream nsh 0.1.1 source and builds the matching CLI. Archives are written to `dist/`. The packaged Rust host registers the same in-process native module as the development host. Installation directories remain read-only.

Checks:

```sh
mkdir -p "$HOME/.cache/nsh-disktree-test-tmp"
export TMPDIR="$HOME/.cache/nsh-disktree-test-tmp"
python3 -m unittest discover -s tests -p 'test_*.py' -v
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --manifest-path native/Cargo.toml --lib --locked
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --manifest-path native/Cargo.toml --lib --locked -- --ignored
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --manifest-path native/Cargo.toml --test ui --locked -- --ignored
nsh check
```

The UI check uses isolated fixture files; it opens and cancels the system Trash confirmation, and never empties your system Trash. Rust tests check system Trash command arguments without executing them. Test logs are written under `target/agents/`. Set `TMPDIR` to a writable directory outside the checkout when running Python unit tests.

When upgrading nsh, update `VERSION` / `REVISION` in `scripts/nsh_toolchain.py` and the exact nsh dependency versions in `native/Cargo.toml`, regenerate `native/Cargo.lock`, then run the checks and packaged-window smoke test. The upstream checkout is never patched; replace an old `.runtime-source` checkout when changing the pin.

## Releases

`.github/workflows/release.yml` builds macOS and Linux archives when a version tag is pushed. It runs scanner regression tests and packaged-window smoke tests, then uploads the archives and `SHA256SUMS` to GitHub Releases.

```sh
git tag v0.1.1
git push origin v0.1.1
```

Manual workflow runs build CI artifacts without publishing a release.

## Measurement

Disk size measures allocated blocks; Apparent measures logical bytes; Files weights the map by file count. Hard-linked bytes are counted once. Symbolic links, other devices, and cloud-only directory placeholders are not traversed. Unreadable paths are reported and make scan totals incomplete. APFS clones are counted independently; system purgeable space and snapshots are not scanned files. Protected macOS locations may require Full Disk Access for the app, or for the terminal during development.

The application is MIT licensed. The nsh runtime is distributed under its accompanying Apache license. The icon in `assets/icon.svg` is original to this project.
