#!/bin/sh
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
python3 scripts/nsh_toolchain.py
export NSH_SOURCE="$PWD/.runtime-source"
exec "${NSH:-nsh}" run "$@"
