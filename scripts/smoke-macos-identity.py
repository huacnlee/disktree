#!/usr/bin/env python3
"""Verify macOS identity for packaged and bare release hosts launched by a terminal."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def main():
    if sys.platform != 'darwin':
        raise SystemExit('This check requires macOS and Xcode Command Line Tools')
    packages = list((ROOT / '.nsh/package').glob('disktree-*/Disktree.app'))
    if len(packages) != 1:
        raise SystemExit(f'Expected one staged app, found {len(packages)}')
    app = packages[0]
    scratch=ROOT/'.nsh/test-data'
    scratch.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='disktree-identity-',dir=scratch) as temporary:
        base = Path(temporary)
        source = base / 'identity.swift'
        source.write_text('''import AppKit
import Foundation
let pid = Int32(CommandLine.arguments[1])!
guard let app = NSRunningApplication(processIdentifier: pid) else { exit(1) }
let info = ["name": app.localizedName ?? "", "identifier": app.bundleIdentifier ?? "", "bundle": app.bundleURL?.path ?? "", "executable": app.executableURL?.path ?? ""]
let data = try JSONSerialization.data(withJSONObject: info, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
''')
        inspector = base / 'identity'
        subprocess.run(['swiftc', str(source), '-o', str(inspector)], check=True)
        # No .app suffix or Info.plist: reproduce direct cargo/binary launches.
        bare = base / 'bare/MacOS/disktree'
        bare.parent.mkdir(parents=True)
        shutil.copy2(app / 'Contents/MacOS/disktree', bare)
        shutil.copytree(app / 'Contents/Resources', base / 'bare/Resources')
        for mode, executable in [('packaged', app / 'Contents/MacOS/disktree'), ('bare', bare)]:
            for inherited in ['com.mitchellh.ghostty', 'com.apple.Terminal']:
                state = base / f'{mode}-{inherited}.json'
                environment = {**os.environ, '__CFBundleIdentifier': inherited,
                               'DISKTREE_ROOT': str(ROOT / 'tests'), 'DISKTREE_STATE': str(state),
                               'DISKTREE_UI_SCRIPT': 'wait 2000; quit'}
                with (base / 'app.log').open('w+') as log:
                    process = subprocess.Popen([str(executable)], env=environment, stdout=log, stderr=log)
                    try:
                        deadline = time.monotonic() + 15
                        while not state.exists() and process.poll() is None and time.monotonic() < deadline:
                            time.sleep(.05)
                        assert state.exists(), 'App did not complete startup scan'
                        info = json.loads(subprocess.check_output([str(inspector), str(process.pid)], text=True))
                        assert info['name'].lower() == 'disktree', info
                        assert Path(info['executable']).resolve() == executable.resolve(), info
                        process_info = subprocess.check_output(['ps', '-p', str(process.pid), '-wwE', '-o', 'command='], text=True)
                        assert f'__CFBundleIdentifier={inherited}' not in process_info, 'Host retained terminal bundle identity'
                        if mode == 'packaged':
                            assert info['identifier'] == 'com.huacnlee.disktree', info
                            assert Path(info['bundle']).resolve() == app.resolve(), info
                        else:
                            assert info['bundle'] not in ['/Applications/Ghostty.app', '/System/Applications/Utilities/Terminal.app'], info
                        assert process.wait(timeout=15) == 0
                        assert json.loads(state.read_text())['directory'] == str(ROOT / 'tests')
                        print(f'{mode}, inherited {inherited}: {info}', flush=True)
                    finally:
                        if process.poll() is None:
                            process.terminate()
                            process.wait(timeout=10)
                        log.seek(0)
                        output = log.read()
                        if output:
                            print(output, flush=True)
    print('macOS app identity checks passed')


if __name__ == '__main__':
    main()
