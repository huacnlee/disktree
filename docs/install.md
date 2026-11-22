# Install Disktree

Disktree has two channels: **Stable** releases and building **from source**.
There is no Nightly channel; every published build is a tagged release.

## Stable

Releases are published at <https://github.com/huacnlee/disktree/releases>.
Published builds support macOS 14 or later on Apple silicon and Intel, and x86_64 Linux.
Other platforms require a [source build](#from-source). Release
archives include the native application with an in-process Rust scanner; Python,
Node, and nsh are not required to run them.

### Install script

```sh
curl -fsSL https://huacnlee.github.io/disktree/install.sh | sh
```

The installer runs as the current user, verifies the release's SHA-256 checksum,
and needs no sudo. It installs at one fixed location, `~/.local/share/disktree`
(`$XDG_DATA_HOME/disktree`), replacing the previous release in place, so only one
version is ever kept. It links the command as `~/.local/bin/disktree`, adds an app
entry to `~/Applications` on macOS, and adds a desktop entry and icon on Linux.

Updater-enabled builds check for new versions quietly on startup. Select
**Update available** in the status bar, download the update, then choose
**Install and restart**. You can also use **View options → Check for updates…**.
The saved scan directory is preserved. Downloads never restart the app without
confirmation.

Older builds without this feature can be upgraded by running the installer
again. It also removes the per-version directories older installers left behind.
In-app updates require the release’s `latest.json`; when unavailable or the
installation is read-only, use the installer or download from GitHub Releases.

| Option | Environment variable | Purpose |
| --- | --- | --- |
| `--version VERSION` | `DISKTREE_VERSION` | Install a specific release, such as `0.1.6` or `v0.1.6` |
| `--dir DIRECTORY` | `DISKTREE_INSTALL_DIR` | Install somewhere else |
| | `DISKTREE_BIN_DIR` | Where to link the `disktree` command |
| | `DISKTREE_APPS_DIR` | Where to link `Disktree.app` on macOS |

Pass options after `sh -s --`:

```sh
curl -fsSL https://huacnlee.github.io/disktree/install.sh | sh -s -- --version 0.1.6
```

The installer refuses to replace a directory that holds anything it did not install.
The installer at `raw.githubusercontent.com/huacnlee/disktree/main/install.sh` is
the same file and keeps working.

### mise

[mise](https://mise.jdx.dev) installs the release from GitHub and selects the
asset for your platform:

```sh
mise use -g github:huacnlee/disktree
```

To install without selecting a global version:

```sh
mise install github:huacnlee/disktree@latest
```

Then run `disktree`.

mise installs under its own directory and does not add the macOS app entry or the
Linux desktop entry. Upgrade with `mise upgrade`.

### Manual download

Download the matching installer `.tar.gz` (without the `-updater` suffix) from
[Releases](https://github.com/huacnlee/disktree/releases), verify it against
`SHA256SUMS`, extract the complete archive, and run `./disktree`. On macOS, you can
also open `Disktree.app`. Keep its accompanying resources together.

## Nightly

Disktree does not publish nightly builds. To try unreleased changes, build the
latest `main` [from source](#from-source).

## From source

Building needs a Rust toolchain. On macOS, install the Xcode Command Line Tools.
On Linux, install the GTK 4 and GStreamer development libraries, for example on
Ubuntu 24.04:

```sh
sudo apt install clang libclang-dev libgtk-4-dev libwebkitgtk-6.0-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

Clone the repository and run the app:

```sh
git clone https://github.com/huacnlee/disktree.git
cd disktree
scripts/setup-nightly.sh
export PATH="$PWD/.nsh/bin:$PATH"
nsh run
```

To build the same
self-contained archives as a release (requires Python 3.11+), see
[Development](../README.md#development).

## Platform notes

**macOS.** Builds use ad hoc signing and are not notarized. First launch may require
allowing the app in System Settings → Privacy & Security. Protected locations may
require Full Disk Access.

**Linux.** Disktree requires GTK 4.14 or later, such as Ubuntu 24.04:

```sh
sudo apt install libgtk-4-1 libglib2.0-bin
```

Put `~/.local/bin` on your `PATH` if the installer reminds you to.

## Uninstall

Close Disktree, then remove what the installer added:

```sh
rm -rf ~/.local/share/disktree ~/.local/bin/disktree
```

On macOS, also remove the app entry:

```sh
rm -f ~/Applications/Disktree.app
```

On Linux, also remove the desktop entry and icon:

```sh
rm -f ~/.local/share/applications/disktree.desktop \
  ~/.local/share/icons/hicolor/scalable/apps/disktree.svg
```

If you set `XDG_DATA_HOME` or the variables above, use those locations instead.
Saved state is separate: `~/Library/Application Support/Disktree` on macOS and
`${XDG_STATE_HOME:-~/.local/state}/nsh-disktree` on Linux.
