#!/bin/sh
# Install a verified prebuilt release. No sudo, Python or nsh required.
set -eu
repo=huacnlee/nsh-disktree
version=${1:-${DISKTREE_VERSION:-latest}}
case $(uname -s) in Darwin) platform=apple-darwin;; Linux) platform=unknown-linux-gnu;; *) echo 'Supported platforms: macOS and Linux' >&2; exit 1;; esac
case $(uname -m) in arm64|aarch64) arch=aarch64;; x86_64|amd64) arch=x86_64;; *) echo 'Unsupported CPU architecture' >&2; exit 1;; esac
fetch() { curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$@"; }
if [ "$version" = latest ]; then
  location=$(fetch -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
  version=${location##*/}
fi
case "$version" in v[0-9]*) ;; [0-9]*) version=v$version;; *) echo 'Expected a release version such as v0.1.0' >&2; exit 1;; esac
case "$version" in *[!a-zA-Z0-9._-]*) echo 'Invalid version' >&2; exit 1;; esac
archive=disktree-$version-$arch-$platform.tar.gz
base=https://github.com/$repo/releases/download/$version
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
fetch "$base/$archive" -o "$scratch/$archive"
fetch "$base/SHA256SUMS" -o "$scratch/SHA256SUMS"
expected=$(awk -v file="$archive" '$2 == file {print $1}' "$scratch/SHA256SUMS")
[ "${#expected}" -eq 64 ] || { echo 'Release checksum is missing' >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$scratch/$archive" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$scratch/$archive" | awk '{print $1}')
fi
[ "$expected" = "$actual" ] || { echo 'Checksum verification failed' >&2; exit 1; }
install_root=${DISKTREE_INSTALL_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/nsh-disktree}
bin_dir=${DISKTREE_BIN_DIR:-$HOME/.local/bin}
mkdir -p "$install_root" "$bin_dir"
release=$(mktemp -d "$install_root/$version-$arch-$platform.XXXXXX")
tar -xzf "$scratch/$archive" -C "$release"
[ -x "$release/disktree" ] || { echo 'Release has no disktree executable' >&2; exit 1; }
ln -sfn "$release/disktree" "$bin_dir/disktree"
if [ "$platform" = apple-darwin ] && [ -d "$release/Disktree.app" ]; then
  apps=${DISKTREE_APPS_DIR:-$HOME/Applications}
  mkdir -p "$apps"
  ln -sfn "$release/Disktree.app" "$apps/Disktree.app"
elif [ -f "$release/disktree.desktop" ]; then
  applications=${XDG_DATA_HOME:-$HOME/.local/share}/applications
  mkdir -p "$applications"
  icons=${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps
  mkdir -p "$icons"
  cp "$release/resources/icon.svg" "$icons/disktree.svg"
  # Quote the executable path so home directories containing spaces work.
  escaped=$(printf '%s' "$release/disktree" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g')
  sed "s|^Exec=.*|Exec=\"$escaped\"|" "$release/disktree.desktop" > "$applications/disktree.desktop"
fi
printf 'Installed Disktree %s to %s\n' "$version" "$bin_dir/disktree"
case :${PATH:-}: in *:"$bin_dir":*) ;; *) printf 'Add %s to your PATH, then run disktree.\n' "$bin_dir";; esac
