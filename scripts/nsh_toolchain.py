"""The upstream release used by JSX tooling and native hosts (no local patches)."""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
VERSION = '0.1.1'
REVISION = '85be2a2e34ab4879acfa74bde3c5bbdc3cc737d0'
SOURCE = ROOT / '.runtime-source'
TARGET = ROOT / '.build/cargo'

def prepare():
    TARGET.mkdir(parents=True, exist_ok=True)
    if not SOURCE.exists():
        subprocess.run(['git', 'clone', '--filter=blob:none', '--branch', 'v'+VERSION,
                        'https://github.com/longbridge/nsh.git', str(SOURCE)], check=True)
    revision = subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != REVISION:
        raise SystemExit(f'{SOURCE} must be the clean upstream nsh v{VERSION} ({REVISION}); found {revision}')
    if subprocess.check_output(['git','-C',str(SOURCE),'status','--porcelain','--untracked-files=no'],text=True).strip():
        raise SystemExit('The nsh checkout has local changes; this project uses unmodified upstream releases')
    # nsh's custom-host builder uses its source checkout's target directory.
    if not (SOURCE / 'target').exists():
        (SOURCE / 'target').symlink_to(TARGET, target_is_directory=True)
    return {**os.environ, 'NSH_SOURCE':str(SOURCE), 'CARGO_TARGET_DIR':str(TARGET)}

if __name__ == '__main__':
    prepare()
    print(f'Prepared upstream nsh {VERSION}. Set this once in your development shell:')
    print(f'export NSH_SOURCE="{SOURCE}"')
    print('Then run: nsh run')
