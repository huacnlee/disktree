#!/usr/bin/env python3
"""Exercise real Disktree GUI packages through signed local updates; no release is published."""
import argparse
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import tarfile
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent
APP_ID = 'com.huacnlee.disktree'


def run(*command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--case', action='append', help='Run only the named case (repeatable)')
    parser.add_argument('--gdb', action='store_true', help='Capture native failure stacks on Linux')
    parser.add_argument('--old', default='0.1.6')
    parser.add_argument('--new', default='0.1.7')
    args = parser.parse_args()
    if not args.skip_build:
        for version in (args.old, args.new):
            run('python3', str(ROOT / 'scripts/build-release.py'), '--version', version)
    openssl = 'openssl'
    if platform.system() == 'Darwin' and shutil.which('brew'):
        prefix = subprocess.check_output(['brew', '--prefix', 'openssl@3'], text=True).strip()
        openssl = str(Path(prefix) / 'bin/openssl')
    arch = 'aarch64' if platform.machine() in ('arm64', 'aarch64') else 'x86_64'
    system = 'apple-darwin' if platform.system() == 'Darwin' else 'unknown-linux-gnu'
    packages = [ROOT / '.nsh/package' / f'disktree-{version}-{arch}-{system}' for version in (args.old, args.new)]
    for package in packages:
        if not (package / 'disktree').is_file():
            raise RuntimeError(f'Missing package: {package}')
    scratch = ROOT / '.nsh/test-data'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='updater smoke ', dir=scratch) as temporary:
        base = Path(temporary)
        # Test-only, ephemeral key. No credentials are committed or published.
        secret = base / 'test-key.pem'
        run(openssl, 'genpkey', '-algorithm', 'ED25519', '-out', str(secret), stdout=subprocess.DEVNULL)
        public_der = subprocess.check_output([openssl, 'pkey', '-in', str(secret), '-pubout', '-outform', 'DER'])
        public = base64.b64encode(public_der[-32:]).decode()
        serving = base / 'server'
        serving.mkdir()

        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(serving), **kwargs)
            def log_message(self, *_):
                pass

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        url = f'http://127.0.0.1:{server.server_port}/latest.json'
        try:
            cases = ['success', 'auto-success', 'bad-signature', 'bad-size', 'bad-hash', 'wrong-app', 'not-executable', 'read-only']
            if system != 'apple-darwin':
                cases.append('launch-failure')
            if os.geteuid() == 0:
                cases.remove('read-only')
            if args.case:
                unknown = set(args.case) - set(cases)
                if unknown:
                    raise ValueError(f'Unknown cases: {unknown}')
                cases = [case for case in cases if case in args.case]
            for case in cases:
                folder = base / case
                installed = folder / 'installation/disktree'
                candidate = folder / 'candidate/disktree'
                shutil.copytree(packages[0], installed)
                shutil.copytree(packages[1], candidate)
                tree = folder / 'files'
                tree.mkdir()
                (tree / 'keep.bin').write_bytes(b'x' * 4096)
                for package, version in ((installed, args.old), (candidate, args.new)):
                    resources = package / 'Disktree.app/Contents/Resources/nsh' if system == 'apple-darwin' else package / 'nsh'
                    (resources / 'assets/nsh-updater.json').write_text(json.dumps({
                        'id': APP_ID, 'version': version, 'url': url, 'publicKey': public,
                    }))
                    if case == 'wrong-app' and package == candidate:
                        (resources / 'app.json').write_text(json.dumps({'id': 'com.example.other', 'version': version}))
                    if system == 'apple-darwin':
                        run('codesign', '--force', '--deep', '--sign', '-', '--timestamp=none', str(package / 'Disktree.app'), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                executable = installed / 'Disktree.app/Contents/MacOS/disktree' if system == 'apple-darwin' else installed / 'disktree'
                new_executable = candidate / 'Disktree.app/Contents/MacOS/disktree' if system == 'apple-darwin' else candidate / 'disktree'
                if case == 'not-executable':
                    new_executable.chmod(0o644)
                if case == 'launch-failure':
                    new_executable.write_bytes(b'not an executable format')
                    new_executable.chmod(0o755)
                archive = serving / 'update.tar.gz'
                with tarfile.open(archive, 'w:gz') as tar:
                    source = candidate / 'Disktree.app' if system == 'apple-darwin' else candidate
                    tar.add(source, arcname=source.name)
                asset = {'url': archive.name, 'size': archive.stat().st_size,
                         'sha256': hashlib.sha256(archive.read_bytes()).hexdigest()}
                manifest = {'id': APP_ID, 'version': args.new, 'platforms': {
                    f'{"macos" if system == "apple-darwin" else "linux"}-{arch}': asset}}
                manifest_path = serving / 'latest.json'
                manifest_path.write_text(json.dumps(manifest))
                framed = base / 'framed'
                framed.write_bytes(b'nsh-updater-manifest\0' + manifest_path.read_bytes())
                signature = base / 'signature'
                run(openssl, 'pkeyutl', '-sign', '-rawin', '-inkey', str(secret), '-in', str(framed), '-out', str(signature))
                (serving / 'latest.json.sig').write_bytes(base64.b64encode(signature.read_bytes()))
                if case == 'bad-signature':
                    manifest['notes'] = 'Changed after signing'
                    manifest_path.write_text(json.dumps(manifest))
                if case == 'bad-size':
                    with archive.open('ab') as file:
                        file.write(b'changed')
                if case == 'bad-hash':
                    with archive.open('r+b') as file:
                        file.seek(20)
                        file.write(bytes([file.read(1)[0] ^ 1]))
                if case == 'read-only':
                    installed.parent.chmod(0o555)
                before = hashlib.sha256(executable.read_bytes()).hexdigest()
                state = folder / 'state.json'
                log_path = folder / 'app.log'
                observations = state.with_suffix('.updater.jsonl')
                def outcomes():
                    return [json.loads(line) for line in observations.read_text().splitlines()] if observations.exists() else []
                state.write_text(json.dumps({'directory': str(tree)}))
                fallback = folder / 'fallback-home'
                fallback.mkdir()
                environment = dict(os.environ, HOME=str(fallback), DISKTREE_STATE=str(state),
                                   DISKTREE_UI_SCRIPT=f'wait 1000; {"update-auto" if case == "auto-success" else "update"}; wait 1000; quit')
                # The restarted app must read saved state, not a forced root.
                # A broken restore can only scan this disposable fallback home.
                environment.pop('DISKTREE_ROOT', None)
                with log_path.open('w') as output:
                    command = [str(executable)]
                    if args.gdb:
                        command = ['gdb', '--batch', '--return-child-result', '-ex', 'set pagination off',
                                   '-ex', 'run', '-ex', 'thread apply all bt', '--args', str(executable)]
                    process = subprocess.Popen(command, env=environment, stdout=output,
                                               stderr=subprocess.STDOUT, start_new_session=True)
                    try:
                        code = process.wait(timeout=90)
                        if case in ('success', 'auto-success'):
                            until = time.monotonic() + 30
                            while not any(event['version'] == args.new and event['outcome'] == 'current' for event in outcomes()):
                                if time.monotonic() > until:
                                    raise AssertionError('New GUI did not restart and report up-to-date')
                                time.sleep(0.1)
                            time.sleep(1.2)  # The restarted GUI completes its quit command.
                            assert code == 0
                            assert subprocess.check_output([str(executable), '--version'], text=True).strip() == f'disktree {args.new}'
                            resources = installed / 'Disktree.app/Contents/Resources/nsh' if system == 'apple-darwin' else installed / 'nsh'
                            assert json.loads((resources / 'app.json').read_text())['version'] == args.new
                            assert hashlib.sha256(executable.read_bytes()).hexdigest() != before
                            assert (installed / '.disktree-install').is_file()
                        else:
                            assert code == 0, (case, code)
                            assert hashlib.sha256(executable.read_bytes()).hexdigest() == before
                            expected = {'bad-signature': 'signature', 'bad-size': 'bytes', 'bad-hash': 'SHA-256',
                                        'wrong-app': 'com.example.other', 'not-executable': 'not executable',
                                        'read-only': 'Download the update', 'launch-failure': 'Exec format error'}[case]
                            assert any(event['outcome'] == 'failed' and expected in event['message'] for event in outcomes()), outcomes()
                        if case not in ('success', 'auto-success'):
                            retry_environment = dict(environment, DISKTREE_UI_SCRIPT='wait 1000; quit')
                            run(str(executable), env=retry_environment, stdout=output, stderr=subprocess.STDOUT, timeout=30)
                        assert json.loads(state.read_text())['directory'] == str(tree)
                        assert (tree / 'keep.bin').stat().st_size == 4096
                        assert not list(installed.parent.glob('.nsh-updater-*'))
                        assert not list(folder.glob('updates/download-*'))
                        print(f'{case}: PASS', flush=True)
                    except Exception:
                        print(log_path.read_text())
                        raise
                    finally:
                        installed.parent.chmod(0o755)
                        try:
                            os.killpg(process.pid, signal.SIGTERM)
                        except ProcessLookupError:
                            pass
        finally:
            server.shutdown()
            server.server_close()
    print('All real-package updater checks passed; installed user copies were untouched.')


if __name__ == '__main__':
    main()
