"""Measure the actual macOS window, scan, and idle period against a memory budget.

Build first, then run:
  python3 scripts/smoke-memory.py --root "$HOME" --budget-mb 150
  python3 scripts/smoke-memory.py --root "$HOME" --rescan-after 35 --timeout 180
Uses isolated preferences; stops only the process it launched.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1] / 'target/release/disktree')
    parser.add_argument('--budget-mb', type=float, default=150)
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--idle-seconds', type=float, default=8)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--rescan-after', type=float, default=0, help='Trigger Cmd+R at this many seconds and measure both scans')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('This check measures the macOS native window with ps and vmmap')
    root = args.root.expanduser().resolve(strict=True)
    peak_rss = 0
    idle_rss = []
    with tempfile.TemporaryDirectory(prefix='disktree-memory-') as temporary:
        state = Path(temporary) / 'state.json'
        rescan_script = f'wait {int(args.rescan_after * 1000)}; key cmd+r; ' if args.rescan_after else ''
        env = {**os.environ, 'DISKTREE_ROOT': str(root), 'DISKTREE_STATE': str(state),
               'DISKTREE_UI_SCRIPT': rescan_script + f'wait {int((args.timeout + 10) * 1000)}; quit'}
        env.pop('__CFBundleIdentifier', None)
        with (Path(temporary) / 'app.log').open('w+') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
            print(f'Memory check PID: {process.pid}; root: {root}', flush=True)
            started = time.monotonic()
            completed = None
            rescan_completed = None
            original_mtime = None
            try:
                while time.monotonic() - started < args.timeout:
                    if process.poll() is not None:
                        raise AssertionError(f'App exited before measurement: {process.returncode}')
                    value = subprocess.check_output(['ps', '-o', 'rss=', '-p', str(process.pid)], text=True).strip()
                    rss = int(value) * 1024
                    peak_rss = max(peak_rss, rss)
                    if state.exists():
                        if completed is None:
                            assert json.loads(state.read_text())['directory'] == str(root)
                            completed = time.monotonic()
                            if args.rescan_after and completed - started >= args.rescan_after:
                                raise AssertionError('First scan exceeded --rescan-after; increase that delay')
                            print(f'Scan completed in {completed - started:.2f}s', flush=True)
                        idle_rss.append(rss)
                        now = time.monotonic()
                        if args.rescan_after:
                            mtime = state.stat().st_mtime_ns
                            if now - started < args.rescan_after:
                                original_mtime = mtime
                            elif mtime != original_mtime and rescan_completed is None:
                                assert json.loads(state.read_text())['directory'] == str(root)
                                rescan_completed = now
                                idle_rss.clear()
                                idle_rss.append(rss)
                                print(f'Rescan completed at {now - started:.2f}s', flush=True)
                            if rescan_completed and now - rescan_completed >= args.idle_seconds:
                                break
                        elif now - completed >= args.idle_seconds:
                            break
                    time.sleep(.1)
                else:
                    raise AssertionError('Scan/idle measurement timed out')
                summary = subprocess.check_output(['vmmap', '-summary', str(process.pid)], text=True)
                footprint = re.search(r'Physical footprint:\s+([0-9.]+)([KMG])', summary)
                peak = re.search(r'Physical footprint \(peak\):\s+([0-9.]+)([KMG])', summary)
                def size(match):
                    assert match, summary
                    return float(match[1]) * {'K': 1024, 'M': 1024**2, 'G': 1024**3}[match[2]]
                result = {'root': str(root), 'scan_seconds': round(completed - started, 2),
                          'peak_sampled_rss_mb': round(peak_rss / 1e6, 2),
                          'idle_rss_mb': round(sorted(idle_rss)[len(idle_rss)//2] / 1e6, 2),
                          'physical_footprint_mb': round(size(footprint) / 1e6, 2),
                          'peak_physical_footprint_mb': round(size(peak) / 1e6, 2),
                          'budget_mb': args.budget_mb}
                if rescan_completed:
                    result['rescan_seconds'] = round(rescan_completed - started - args.rescan_after, 2)
                print(json.dumps(result, indent=2), flush=True)
                if args.report:
                    args.report.parent.mkdir(parents=True, exist_ok=True)
                    args.report.write_text(json.dumps(result, indent=2) + '\n')
                    args.report.with_suffix('.vmmap.txt').write_text(summary)
                assert peak_rss < args.budget_mb * 1e6, 'Process RSS exceeded memory budget'
                assert size(peak) < args.budget_mb * 1e6, 'Physical footprint exceeded memory budget'
                print('Memory budget passed', flush=True)
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=10)
                log.seek(0)
                output = log.read()
                if output:
                    print(output, flush=True)


if __name__ == '__main__':
    main()
