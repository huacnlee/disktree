"""Resolve JSX tooling from the same Git checkout as the native runtime."""
import json
import os
from pathlib import Path
import subprocess
import sys
import shutil
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / '.build/cargo'

def prepare():
    target = Path(os.environ.get('CARGO_TARGET_DIR', str(TARGET))).resolve()
    target.mkdir(parents=True, exist_ok=True)
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--manifest-path', str(ROOT / 'Cargo.toml'),
         '--format-version', '1', '--locked'], cwd=ROOT, text=True))
    runtime = next(package for package in metadata['packages']
                   if package['name'] == 'nsh-platform-macos')
    source = Path(runtime['manifest_path']).parents[2]
    # NSH's generated custom hosts use the source checkout's target directory.
    if not (source / 'target').exists():
        (source / 'target').symlink_to(target, target_is_directory=True)
    # Newer CLIs infer registry dependencies from Git-based app manifests.
    # Apply the app's Git source to their generated Cargo workspaces as well.
    config = target / 'nsh-source.toml'
    patches = []
    for manifest in sorted((source / 'crates').glob('*/Cargo.toml')):
        package = tomllib.loads(manifest.read_text()).get('package', {})
        name = package.get('name', '')
        if name.startswith('nsh-'):
            patches.append(f'{name} = {{ path = {json.dumps(str(manifest.parent))} }}')
    entries = '\n'.join(patches)
    config.write_text(f'[patch.crates-io]\n{entries}\n\n[patch."https://github.com/longbridge/nsh.git"]\n{entries}\n')
    wrapper = target / 'nsh-cargo'
    cargo = shutil.which('cargo')
    wrapper.write_text(f'#!{sys.executable}\nimport os, sys\nos.execv({cargo!r}, [{cargo!r}, "--config", {str(config)!r}, *sys.argv[1:]])\n')
    wrapper.chmod(0o755)
    return {**os.environ, 'NSH_SOURCE': str(source),
            'NSH_VERSION': runtime['version'], 'CARGO_TARGET_DIR': str(target),
            'CARGO': str(wrapper), 'NSH_CARGO': str(wrapper)}

if __name__ == '__main__':
    environment = prepare()
    if len(sys.argv) > 1:
        raise SystemExit(subprocess.run(sys.argv[1:], env=environment, cwd=ROOT).returncode)
    print(f"NSH_SOURCE={environment['NSH_SOURCE']}")
