"""Resolve JSX tooling from the same Git checkout as the native runtime."""
import json
import os
from pathlib import Path
import subprocess
import sys

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
    return {**os.environ, 'NSH_SOURCE': str(source),
            'NSH_VERSION': runtime['version'], 'CARGO_TARGET_DIR': str(target)}

if __name__ == '__main__':
    environment = prepare()
    if len(sys.argv) > 1:
        raise SystemExit(subprocess.run(sys.argv[1:], env=environment, cwd=ROOT).returncode)
    print(f"NSH_SOURCE={environment['NSH_SOURCE']}")
