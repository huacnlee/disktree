#!/usr/bin/env python3
"""Build an installable, self-contained nsh app for the current platform."""
import argparse, hashlib, os, platform, plistlib, shutil, subprocess, sys, tarfile
from pathlib import Path
from nsh_toolchain import prepare, VERSION as NSH_VERSION

ROOT=Path(__file__).resolve().parent.parent
def run(*command,**kwargs):
    subprocess.run(command,cwd=ROOT,check=True,**kwargs)
def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--version',default='0.1.1')
    args=parser.parse_args()
    version=args.version.removeprefix('v')
    system=platform.system()
    if system not in ('Darwin','Linux'):raise SystemExit('Build on macOS or Linux')
    target=f"{'aarch64' if platform.machine() in ('arm64','aarch64') else 'x86_64'}-{'apple-darwin' if system=='Darwin' else 'unknown-linux-gnu'}"
    toolchain=prepare()
    source=Path(toolchain['NSH_SOURCE'])
    build=ROOT/'.build'
    build.mkdir(exist_ok=True)
    cargo=Path(os.environ.get('CARGO_TARGET_DIR',str(build/'cargo'))).resolve()
    env={**toolchain,'CARGO_TARGET_DIR':str(cargo),'DISKTREE_VERSION':version}
    nsh=os.environ.get('NSH')
    if not nsh:
        run('cargo','build','--manifest-path',str(source/'Cargo.toml'),'--release','--locked','-p','nsh-cli',env=env)
        nsh=str(cargo/'release/nsh')
    actual=subprocess.check_output([nsh,'--version'],text=True).strip()
    if actual != 'nsh '+NSH_VERSION:
        raise SystemExit(f'Expected nsh {NSH_VERSION}, found {actual}')
    run(nsh,'check',env=env)
    run(nsh,'build','-o',str(build/'bundle'),env=env)
    run('cargo','build','--manifest-path',str(ROOT/'native/Cargo.toml'),'--release','--locked',env=env)
    stage=build/f'disktree-{version}-{target}'
    if stage.exists():shutil.rmtree(stage)
    stage.mkdir()
    if system=='Darwin':
        app=stage/'Disktree.app/Contents'
        (app/'MacOS').mkdir(parents=True)
        resources=app/'Resources';resources.mkdir()
        shutil.copy2(cargo/'release/disktree',app/'MacOS/disktree')
        with (app/'Info.plist').open('wb') as file:
            plistlib.dump(dict(CFBundleExecutable='disktree',CFBundleName='Disktree',CFBundleIconFile='AppIcon',CFBundleIdentifier='com.huacnlee.disktree',CFBundleShortVersionString=version,CFBundleVersion=version,CFBundlePackageType='APPL',LSMinimumSystemVersion='14.0',NSHighResolutionCapable=True),file)
        run('sips','-s','format','png',str(ROOT/'assets/icon.svg'),'--out',str(build/'icon.png'))
        iconset=build/'AppIcon.iconset'
        if iconset.exists():shutil.rmtree(iconset)
        iconset.mkdir()
        for size in [16,32,128,256,512]:
            for scale in [1,2]:
                name=f'icon_{size}x{size}'+('@2x' if scale==2 else '')+'.png'
                run('sips','-z',str(size*scale),str(size*scale),str(build/'icon.png'),'--out',str(iconset/name))
        run('iconutil','-c','icns',str(iconset),'-o',str(resources/'AppIcon.icns'))
        launcher=stage/'disktree'
        launcher.write_text('''#!/bin/sh
set -eu
launcher=$0
while [ -L "$launcher" ]; do
  link=$(readlink "$launcher")
  case "$link" in /*) launcher=$link;; *) launcher=$(dirname "$launcher")/$link;; esac
done
root=$(CDPATH= cd -- "$(dirname -- "$launcher")" && pwd)
exec "$root/Disktree.app/Contents/MacOS/disktree" "$@"
''')
        launcher.chmod(0o755)
    else:
        resources=stage/'resources';resources.mkdir()
        shutil.copy2(cargo/'release/disktree',stage/'disktree')
        (stage/'disktree.desktop').write_text('[Desktop Entry]\nType=Application\nName=Disktree\nComment=Explore disk space with a native treemap\nExec=disktree\nTerminal=false\nCategories=System;Utility;\nIcon=disktree\n')
    shutil.copy2(ROOT/'assets/icon.svg',resources/'icon.svg')
    shutil.copy2(build/'bundle/bundle.js',resources/'bundle.js')
    shutil.copy2(ROOT/'LICENSE',stage/'LICENSE')
    shutil.copy2(source/'LICENSE-APACHE',stage/'LICENSE.nsh')
    if system=='Darwin':run('codesign','--force','--deep','--sign','-','--timestamp=none',str(stage/'Disktree.app'))
    # Validate the CLI wrapper before archiving; smoke-package checks native scanning.
    run(str(stage/'disktree'),'--version')
    dist=ROOT/'dist';dist.mkdir(exist_ok=True)
    archive=dist/f'disktree-v{version}-{target}.tar.gz'
    with tarfile.open(archive,'w:gz') as tar:
        for child in sorted(stage.iterdir()):tar.add(child,arcname=child.name)
    print('Built',archive)
    print('SHA256',hashlib.sha256(archive.read_bytes()).hexdigest())
if __name__=='__main__':main()
