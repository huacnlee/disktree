# Disktree

A native disk-space explorer built with **nsh**, inspired by [tobi/disktree](https://github.com/tobi/disktree). Runs on macOS and Linux with an original capacity-ring icon and a black interface with subtle dimensional surfaces and vivid category colors.

<img width="1303" height="893" alt="image" src="https://github.com/user-attachments/assets/c47b62b3-f03a-4d82-a33a-7197c254aaff" />

- Scans immediately on launch and remembers the last directory.
- Nested, color-coded treemap for code, Git, caches, builds, toolchains, media, and more.
- Native treemap transitions and mint-to-warm-gold outline around the map.
- Native MacBook pinch gestures and mouse dragging; keyboard and button zoom controls.
- Click to select; click a selected folder again to enter it. Breadcrumbs navigate with one click; long paths collapse intermediate folders into a menu instead of scrolling.
- Point at a tile and press Space to toggle its removal mark.
- Amber fill, diagonal hatching, and check badges distinguish marked files and folders.
- Review full paths before moving items to the system Trash. Permanent deletion and emptying the system Trash require separate confirmation.
- Large scan indexes stay in Rust; the UI reads bounded directory pages and map previews.

## Install

Release archives include the native application with an in-process Rust scanner. Python, Node, and nsh are not required to run them.

### mise

```sh
mise use -g github:huacnlee/disktree
# Install without selecting a global version:
# mise install github:huacnlee/disktree@latest
disktree
```

The [mise GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html) selects the release asset for your platform.

### curl

```sh
curl -fsSL https://raw.githubusercontent.com/huacnlee/disktree/main/install.sh | sh
```

To install a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/huacnlee/disktree/main/install.sh | sh -s -- v0.1.0
```

The installer verifies the release's SHA-256 checksum. It installs the command in `~/.local/bin/disktree`, with a macOS app entry in `~/Applications`. Override these locations with `DISKTREE_BIN_DIR`, `DISKTREE_INSTALL_DIR`, and `DISKTREE_APPS_DIR`. Run the installer again to upgrade.

### Manual download

Download the matching `.tar.gz` from [Releases](https://github.com/huacnlee/disktree/releases), extract the complete archive, and run `./disktree`. On macOS, you can also open `Disktree.app`. Keep its accompanying resources together.

macOS requires 14 or later. Builds use ad hoc signing and are not notarized; first launch may require allowing the app in System Settings → Privacy & Security. Linux requires GTK 4.14 or later, such as Ubuntu 24.04:

```sh
sudo apt install libgtk-4-1 libglib2.0-bin
```

## Controls

| Action | Control |
| --- | --- |
| Scan a folder | Click ↻; use ✎ to edit a path |
| Show hidden files / set map depth | Settings icon → Show hidden files / Treemap depth |
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

Install Rust and the platform build libraries listed below, then run from the repository root:

```sh
cargo run
```

The Rust launcher compiles the current TSX sources and opens the native window with the in-process scanner. On macOS, the executable embeds Disktree application metadata and clears an inherited terminal bundle identity before initializing AppKit, so direct launches appear as disktree instead of Ghostty or Terminal. The packaged `.app` displays Disktree with its own icon. Run again to load source changes. Development and installed applications use the same native module; Python is used only for test and packaging tools.

NSH dependencies follow Git `main`, with the resolved commit recorded in the root `Cargo.lock`. The optional NSH CLI workflow supports JSX hot reload and rebuilds the host on Rust changes. Use `nsh dev` to start the hot reload server, or `nsh run --target macos` to also open the AppKit window. The CLI resolves the Git runtime from the project’s Cargo dependencies and uses it for generated hosts.

Build a self-contained application with Rust, Python 3.11+ (packaging tools only), and Xcode Command Line Tools on macOS, or `clang libclang-dev libgtk-4-dev` on Linux:

```sh
python3 scripts/build-release.py --version 0.1.1
```

The first build downloads NSH through Cargo and builds the matching CLI from the runtime’s Git checkout. Release builds disable the development bundler and load the packaged bundle. Archives are written to `dist/`. The packaged Rust host registers the same in-process native module as the development host. Installation directories remain read-only.

Checks:

```sh
mkdir -p "$HOME/.cache/nsh-disktree-test-tmp"
export TMPDIR="$HOME/.cache/nsh-disktree-test-tmp"
python3 -m unittest discover -s tests -p 'test_*.py' -v
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --manifest-path native/Cargo.toml --lib --locked
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --manifest-path native/Cargo.toml --lib --release --locked -- --ignored --test-threads=1
python3 scripts/nsh_toolchain.py cargo test --manifest-path native/Cargo.toml --test ui --locked -- --ignored
python3 scripts/nsh_toolchain.py nsh check
```

The UI check uses isolated fixture files; it opens and cancels the system Trash confirmation, and never empties your system Trash. Rust tests check system Trash command arguments without executing them. Test logs are written under `target/agents/`. Set `TMPDIR` to a writable directory outside the checkout when running Python unit tests.

To update NSH, run `cargo update -p nsh-platform-macos -p nsh-platform-linux -p nsh-bundler -p nsh-core`, then run the checks and packaged-window smoke test. Cargo keeps the runtime and bundler on the same Git revision; the toolchain helper uses that checkout for the CLI.

## Releases

`.github/workflows/release.yml` builds macOS and Linux archives when a version tag is pushed. It runs scanner regression tests and packaged-window smoke tests, then uploads the archives and `SHA256SUMS` to GitHub Releases.

```sh
git tag v0.1.1
git push origin v0.1.1
```

Manual workflow runs build CI artifacts without publishing a release.

## Performance (release build)

### Current release after the fixes

Measured on the same machine with the memory, application identity, and treemap animation fixes applied on top of `634ad4d`. Workloads and sampling follow the baseline method below; values are medians with observed ranges. This measurement predates the subsequent static gradient-border styling. There are three successful trials each for the empty and 16,384-file trees, and two for the 131,072-file tree. Sessions in which navigation changed the test directory were excluded from the idle comparison.

| Metric | Empty directory | 16,384 files | 131,072 files |
| --- | --- | --- | --- |
| Window appears | 188 ms (180–198) | 242 ms (231–249) | 268 ms (216–320) |
| Initial scan saved | 115 ms (105–131) | 136 ms (132–139) | 278 ms (234–321) |
| Peak process RSS | 98.0 MiB (97.9–98.1) | 129.0 MiB (128.3–129.3) | 162.5 MiB (161.1–163.9) |
| Idle process RSS | 97.7 MiB (97.7–97.8) | 128.4 MiB (127.8–128.7) | 162.3 MiB (160.8–163.7) |
| Average CPU, first ≈3 seconds | 5.3% (4.7–5.4) | 10.8% (10.8–12.4) | 20.3% (19.9–20.7) |
| Average CPU, seconds 5–10.5 | 0.00% (0.00–0.18) | 0.00% (0.00–0.00) | 0.09% (0.00–0.19) |

For 131,072 files, idle process RSS decreased from 220.4 MiB to 162.3 MiB (about 26%). Idle CPU remained 0–0.19% across the valid runs. The fixture still leaves the complete index in memory after scanning; multi-million-entry trees will require more memory. Linux was not measured locally.

### Baseline before memory and animation fixes

Measured on 2026-10-20 at commit `634ad4d`: Apple M3 MacBook Pro (8 CPU cores, 24 GB RAM), macOS 27.2, local APFS storage, Rust 1.99.0. The app was rebuilt with `--release --locked --no-default-features` and a freshly packaged JSX bundle. Measurements launch the packaged executable directly, excluding compilation, packaging, and development-time bundling.

Each workload uses a fresh process and isolated `DISKTREE_STATE`, with three runs. The synthetic directories contain short, uniform names and one-byte files: 64 folders × 256 files, or 512 folders × 256 files; folder counts exclude the root. Fixtures were created immediately before testing, so filesystem caches are warm. Values below are medians, with observed ranges in parentheses.

| Metric | Empty directory | 16,384 files | 131,072 files |
| --- | --- | --- | --- |
| Window appears | 203 ms (183–527) | 244 ms (243–249) | 215 ms (184–240) |
| Initial scan saved | 114 ms (108–272) | 132 ms (116–137) | 252 ms (207–254) |
| Peak process RSS | 97.8 MiB (97.7–97.9) | 133.8 MiB (132.9–134.5) | 220.5 MiB (219.3–221.3) |
| Idle process RSS | 97.4 MiB (97.3–97.5) | 133.1 MiB (132.1–133.7) | 220.4 MiB (219.2–221.1) |
| Average CPU, first ≈3 seconds | 5.7% (5.3–9.8) | 9.8% (9.7–10.1) | 18.5% (18.4–21.6) |
| Average CPU, seconds 5–10.5 | 0.18% (0–0.19) | 0% (0–0.19) | 0% (0–0.19) |

Timing starts immediately before process creation. Window appearance means the first on-screen, layer-zero window reported by Core Graphics; it does not measure fully rendered content or input readiness. Initial scan completion is detected when the isolated state file is written, after native scanning and before the subsequent UI view request. Both are polled approximately every 10 ms, with additional sampling overhead. A scan can finish before its window appears. The UI also has a 680 ms entrance animation, which these startup timings do not wait for.

RSS is resident process memory, not macOS Activity Monitor's memory footprint. Peak RSS comes from `wait4().ru_maxrss`; idle RSS is the median of `ps -o rss=,time=` samples taken approximately every 100 ms during seconds 5–10.5. CPU is the change in cumulative user + system CPU time divided by elapsed wall time: **100% means one fully occupied core**. The first-three-second average includes startup, scanning, rendering, animation, and some idle time; it is not peak scanning CPU. Each app exits through `DISKTREE_UI_SCRIPT='wait 11000; quit'`. All nine runs exited successfully and saved the expected directory.

The existing scanner-only benchmark, also run in release mode, scanned 16,384 empty files and 65 directories in **7.24 ms with four readers**, versus **16.93 ms with one reader** (2.34× speedup). These are medians of three timed iterations after one warm-up per configuration, excluding fixture creation and GUI startup. Reproduce the scanner benchmark with:

```sh
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --release --no-default-features --locked --lib scan_benchmark -- --ignored --nocapture
# Optional: benchmark an existing directory instead of the generated fixture.
# DISKTREE_BENCH_ROOT=/path/to/tree CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --release --no-default-features --locked --lib scan_benchmark -- --ignored --nocapture
```

### Large-tree observation

A separate live inspection of a user-launched release process on the same machine (`vmmap -summary`, 2026-10-20) found approximately **2.22 GiB process RSS**, **3.0 GiB physical footprint**, and a **3.4 GiB peak physical footprint**. The default malloc zone reported **2.8 GiB allocated across about 20.1 million allocations**, with about 900 MiB in its swapped column and 4% fragmentation. This was an existing large-tree session, not a controlled repeat of the synthetic workloads above; its exact final entry count and scan history were not captured. The small-fixture results therefore substantially understate large-tree memory requirements.

Before the fix, each entry's full path was independently allocated in `Node.path`, the `nodes` map key, and the `records` map key. The updated index shares one `Arc<str>` path between nodes and lookup keys, and stores removal-validation metadata directly in each node, eliminating the second hash table. Removal checks still compare exact device, inode, size, and nanosecond timestamps. The heap summary above is a pre-fix observation; it does not measure the current build or attribute individual allocations to Rust versus QuickJS.

### Index memory regression check

A deterministic release benchmark builds the real native index with 131,072 files, 513 directories, and long paths, without filesystem or GUI overhead. macOS default-zone allocator statistics are sampled before and after index construction:

| Retained index cost | Before | After | Reduction |
| --- | --- | --- | --- |
| Allocated heap | 127.95 MiB | 57.53 MiB | 55% |
| Live allocations | ≈659,700 | ≈396,500 | 40% |

This measures retained native heap, not process RSS or physical footprint; the reduction cannot be applied directly to the multi-million-entry session above. A regression gate requires this fixture to stay below 80 MiB. Run it alone so other test allocations cannot affect the measurement:

```sh
CARGO_TARGET_DIR="$PWD/.build/cargo" cargo test --release --no-default-features --locked --lib scanner::tests::long_path_index_memory_budget -- --ignored --exact --nocapture
```

The existing scan benchmark after the change measured 9.00 ms with four readers and 18.57 ms with one reader (2.06×). These local timings vary with filesystem state and other activity.

Treemap layout changes now request finite native transitions: 320 ms for insertion/repositioning/resizing and 160 ms for removal, respecting Reduce Motion. No animation frame loop runs in JavaScript. The UI regression test verifies that transitions are requested when changing metrics and navigating. Native screenshots change during the transition and remain identical once it settles. The macOS identity smoke check exercises both packaged and bare executable launches with inherited Ghostty and Terminal bundle IDs:

```sh
# After building the release package:
python3 scripts/smoke-macos-identity.py
```

The measurements and implementation suggest:

- **Memory grows with indexed entries.** The complete Rust index retains nodes, paths, metadata records, and child references. In the updated build, the 131,072-file fixture adds about 65 MiB of idle RSS above the empty-directory baseline. This includes allocator and UI effects, so it is not a per-file allocation estimate. Longer paths, more directories, hard links, and rescan overlap can change memory use. UI pagination (80 items) and map previews (240-node budget) bound the data sent to JSX, but do not bound the native index.
- **CPU work is concentrated around scanning and rendering.** Native scanning runs off the UI thread with up to four directory readers, bounded batches, and a single reducer that assembles and sorts the index. macOS uses bulk directory metadata reads. Scan-progress polling stops after scanning; the measured idle CPU is close to zero. Zooming, dragging, repeated rescans, and sustained interaction were not benchmarked.
- **Startup is fast on these warm fixtures.** Packaging removes runtime JSX compilation, and automatic scanning begins during UI initialization. The slowest baseline window observation was the first empty-directory launch (527 ms); these small sample counts are insufficient for a tail-latency claim. Cold boot, first installation/Gatekeeper checks, large real-world trees, network disks, memory pressure, and Linux need separate measurements. These results do not establish a memory-leak bound or performance guarantee.

## Disk size measurement

Disk size measures allocated blocks; Apparent measures logical bytes; Files weights the map by file count. Hard-linked bytes are counted once. Symbolic links, other devices, and cloud-only directory placeholders are not traversed. Unreadable paths are reported and make scan totals incomplete. APFS clones are counted independently; system purgeable space and snapshots are not scanned files. Protected macOS locations may require Full Disk Access for the app, or for the terminal during development.

The application is MIT licensed. The nsh runtime is distributed under its accompanying Apache license. The icon in `assets/icon.svg` is original to this project.
