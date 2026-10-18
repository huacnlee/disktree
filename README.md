# Disktree

A native disk-space explorer built with **nsh**, inspired by [tobi/disktree](https://github.com/tobi/disktree). Runs on macOS and Linux with an original capacity-ring icon and a black interface with subtle dimensional surfaces and vivid category colors.

<img width="1303" height="893" alt="image" src="https://github.com/user-attachments/assets/c47b62b3-f03a-4d82-a33a-7197c254aaff" />

- Scans immediately on launch and remembers the last directory.
- Nested, color-coded treemap for code, Git, caches, builds, toolchains, media, and more.
- Native MacBook pinch gestures and mouse dragging; keyboard and button zoom controls.
- Click to select; click a selected folder again to enter it. Breadcrumbs navigate with one click.
- Point at a tile and press Space to toggle its removal mark.
- Amber fill, diagonal hatching, and check badges distinguish marked files and folders.
- Review full paths before moving items to the system Trash. The app never empties Trash.
- Large scan indexes stay in the local service; the UI reads bounded directory pages and map previews.

## Install

Release archives include the native application and its scanner. Python, Node, and nsh are not required to run them.

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
| Scan a folder | Enter a path, then press Enter or Scan folder |
| Select a tile | Click, or use the arrow keys |
| Enter a folder | Click the selected folder again, Enter, or Open folder |
| Navigate back | Click a breadcrumb, or Alt+↑ |
| Zoom | Pinch out/in, use +/− buttons, or Cmd/Ctrl + / - |
| Pan | Drag the map with the mouse |
| Fit the map | Click the percentage or press Cmd/Ctrl+0 |
| Mark/unmark | Point at a tile and press Space, or use Mark / unmark |
| Review removal | Review marked items → inspect paths → Confirm move to Trash |

Marking does not delete anything. Rescan after moving files to update the map. Disk space is reclaimed only after the Trash is emptied outside this app.

Directory lists contain 80 items per page. Filtering searches all direct children of the current directory, including other pages. Aggregate tiles preserve the weight of undisplayed items and cannot be marked. Enter individual folders to inspect their contents.

The last directory is saved in `~/Library/Application Support/Disktree/state.json` on macOS and `${XDG_STATE_HOME:-~/.local/state}/nsh-disktree/state.json` on Linux. `DISKTREE_ROOT` overrides the initial directory; `DISKTREE_STATE` overrides the state file.

## Development

Install [nsh](https://github.com/longbridge/nsh) and Python 3, then start both the scanner and development host:

```sh
./scripts/run.sh
```

Running `nsh run` alone does not start the scanner. The script manages both processes. Packaged applications start their own scanner automatically.

The native launcher depends on NSH's Git `main` branch. Cargo records the resolved commit in `native/Cargo.lock`; update it with `cargo update --manifest-path native/Cargo.toml -p nsh-platform-macos -p nsh-platform-linux`. Desktop hover events are included in NSH.

Build a self-contained application with Rust, Python 3.11+, and Xcode Command Line Tools on macOS, or `clang libclang-dev libgtk-4-dev` on Linux:

```sh
python3 -m venv .venv
.venv/bin/pip install pyinstaller==6.16.0
.venv/bin/python scripts/build-release.py --version 0.1.0
```

The first build downloads NSH through Cargo. The packaging script builds the CLI from the same Git checkout as the runtime. Archives are written to `dist/`. A thin Rust launcher starts the bundled scanner, reads its random port and authentication token, and injects the connection before loading the nsh JS bundle. Installation directories remain read-only.

Checks:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py' -v
python3 scripts/stress_memory.py 650000 6
nsh check
```

`connection.ts` is generated locally and is not committed. Before running a standalone type check, copy `connection.example.ts` to `connection.ts` if the generated file does not exist.

## Releases

`.github/workflows/release.yml` builds macOS and Linux archives when a version tag is pushed. It runs scanner regression tests and packaged-window smoke tests, then uploads the archives and `SHA256SUMS` to GitHub Releases.

```sh
git tag v0.1.0
git push origin v0.1.0
```

Manual workflow runs build CI artifacts without publishing a release.

## Measurement

Disk size measures allocated blocks; Apparent measures logical bytes; Files weights the map by file count. Hard-linked bytes are counted once. Symbolic links, other devices, and cloud-only directory placeholders are not traversed. Unreadable paths are reported and make scan totals incomplete. APFS clones are counted independently; system purgeable space and snapshots are not scanned files. Protected macOS locations may require Full Disk Access for the app, or for the terminal during development.

The application is MIT licensed. The nsh runtime is distributed under its accompanying Apache license. The icon in `assets/icon.svg` is original to this project.
