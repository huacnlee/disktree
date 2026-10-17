#!/bin/sh
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
rm -f connection.ts
python3 scripts/service.py &
scanner_pid=$!
trap 'kill "$scanner_pid" 2>/dev/null || true' EXIT INT TERM
attempt=0
while [ ! -f connection.ts ]; do
  kill -0 "$scanner_pid" 2>/dev/null || exit 1
  attempt=$((attempt+1))
  [ "$attempt" -lt 100 ] || { echo 'Scanner startup timed out' >&2; exit 1; }
  sleep 0.05
done
case $(uname -s) in Darwin) target=macos;; Linux) target=linux;; *) echo 'Use macOS or Linux' >&2;exit 1;; esac
"${NSH:-nsh}" run --target "$target"
