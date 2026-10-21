#!/bin/sh
# Install a verified prebuilt release. No sudo, Python or nsh required.
set -eu
repo=huacnlee/disktree
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
staging=
retired=
cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  [ -z "$staging" ] || rm -rf -- "$staging"
  [ -z "$retired" ] || rm -rf -- "$retired"
  rm -rf -- "$scratch"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
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
data_home=${XDG_DATA_HOME:-$HOME/.local/share}
install_root=${DISKTREE_INSTALL_DIR:-$data_home/disktree}
bin_dir=${DISKTREE_BIN_DIR:-$HOME/.local/bin}
marker=.disktree-install
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
# Every release lives at the same fixed path, so launchers, the Dock and
# Spotlight only ever see one Disktree. Older installers kept one directory per
# version; those are installer-owned and removed during the upgrade.
versioned() {
  case "${1##*/}" in
    v[0-9]*-aarch64-apple-darwin.??????|v[0-9]*-x86_64-apple-darwin.??????|v[0-9]*-aarch64-unknown-linux-gnu.??????|v[0-9]*-x86_64-unknown-linux-gnu.??????)
      [ -d "$1" ] && [ ! -L "$1" ];;
    *) return 1;;
  esac
}
forget_app() {
  if [ -x "$lsregister" ] && [ -d "$1/Disktree.app" ]; then
    "$lsregister" -u "$1/Disktree.app" >/dev/null 2>&1 || :
  fi
}
prune_versions() {
  [ -d "$1" ] && [ ! -L "$1" ] || return 0
  for old in "$1"/v*; do
    versioned "$old" || continue
    forget_app "$old"
    rm -rf -- "$old"
  done
}
# Replace an existing root only when this installer owns everything in it.
owned_root() {
  [ -d "$1" ] && [ ! -L "$1" ] || return 1
  [ -f "$1/$marker" ] && return 0
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    [ "${entry##*/}" = .DS_Store ] && continue
    versioned "$entry" || return 1
  done
}
mkdir -p "$bin_dir"
parent=$(dirname -- "$install_root")
mkdir -p "$parent"
parent=$(CDPATH= cd -- "$parent" && pwd -P)
install_root=$parent/${install_root##*/}
if [ -e "$install_root" ] || [ -L "$install_root" ]; then
  owned_root "$install_root" || { printf '%s is not a Disktree installation; refusing to replace it\n' "$install_root" >&2; exit 1; }
fi
# Unpack beside the destination so activation is a rename on one filesystem.
staging=$(mktemp -d "$parent/.disktree-staging.XXXXXX")
tar -xzf "$scratch/$archive" -C "$staging"
[ -x "$staging/disktree" ] || { echo 'Release has no disktree executable' >&2; exit 1; }
printf '%s %s-%s\n' "$version" "$arch" "$platform" > "$staging/$marker"
if [ -d "$install_root" ]; then
  prune_versions "$install_root"
  forget_app "$install_root"
  retired=$(mktemp -d "$parent/.disktree-retired.XXXXXX")
  mv -- "$install_root" "$retired/"
  if ! mv -- "$staging" "$install_root"; then
    mv -- "$retired/${install_root##*/}" "$install_root"
    echo 'Could not activate the new release' >&2
    exit 1
  fi
else
  mv -- "$staging" "$install_root"
fi
staging=
ln -sfn "$install_root/disktree" "$bin_dir/disktree"
if [ "$platform" = apple-darwin ] && [ -d "$install_root/Disktree.app" ]; then
  apps=${DISKTREE_APPS_DIR:-$HOME/Applications}
  mkdir -p "$apps"
  ln -sfn "$install_root/Disktree.app" "$apps/Disktree.app"
  [ ! -x "$lsregister" ] || "$lsregister" -f "$install_root/Disktree.app" >/dev/null 2>&1 || :
elif [ -f "$install_root/disktree.desktop" ]; then
  applications=$data_home/applications
  mkdir -p "$applications"
  icons=$data_home/icons/hicolor/scalable/apps
  mkdir -p "$icons"
  cp "$install_root/resources/icon.svg" "$icons/disktree.svg"
  # Quote the executable path so home directories containing spaces work.
  escaped=$(printf '%s' "$install_root/disktree" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g')
  sed "s|^Exec=.*|Exec=\"$escaped\"|" "$install_root/disktree.desktop" > "$applications/disktree.desktop"
fi
if [ -z "${DISKTREE_INSTALL_DIR:-}" ]; then
  legacy=$data_home/nsh-disktree
  if [ -d "$legacy" ] && [ ! -L "$legacy" ]; then
    prune_versions "$legacy"
    if [ -f "$legacy/.DS_Store" ] && [ ! -L "$legacy/.DS_Store" ]; then
      rm -f -- "$legacy/.DS_Store"
    fi
    rmdir "$legacy" 2>/dev/null || :
  fi
fi
printf 'Installed Disktree %s to %s\n' "$version" "$install_root"
case :${PATH:-}: in *:"$bin_dir":*) ;; *) printf 'Add %s to your PATH, then run disktree.\n' "$bin_dir";; esac
