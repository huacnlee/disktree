#!/usr/bin/env python3
"""Build an installable, self-contained nsh app for the current platform."""
import argparse, hashlib, os, platform, plistlib, shutil, subprocess, sys, tarfile
from pathlib import Path

ROOT=Path(__file__).resolve().parent.parent
RUNTIME_REV='6d62ee0efdc1466337ce5a6a1a02e5aa456dc71c'
def run(*command,**kwargs):
    subprocess.run(command,cwd=ROOT,check=True,**kwargs)
def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--version',default='0.1.0')
    args=parser.parse_args()
    version=args.version.removeprefix('v')
    system=platform.system()
    if system not in ('Darwin','Linux'):raise SystemExit('Build on macOS or Linux')
    target=f"{'aarch64' if platform.machine() in ('arm64','aarch64') else 'x86_64'}-{'apple-darwin' if system=='Darwin' else 'unknown-linux-gnu'}"
    source=ROOT/'.runtime-source'
    if not source.exists():
        run('git','clone','--filter=blob:none','https://github.com/longbridge/native-shell.git',str(source))
        run('git','-C',str(source),'checkout',RUNTIME_REV)
    revision=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()
    if revision!=RUNTIME_REV:raise SystemExit('Unexpected nsh revision: '+revision)
    patch=ROOT/'patches/nsh-hover.patch'
    if subprocess.run(['git','-C',str(source),'apply','--check',str(patch)],capture_output=True).returncode==0:
        run('git','-C',str(source),'apply',str(patch))
    else:
        run('git','-C',str(source),'apply','--reverse','--check',str(patch))
    build=ROOT/'.build'
    build.mkdir(exist_ok=True)
    cargo=Path(os.environ.get('CARGO_TARGET_DIR',str(build/'cargo'))).resolve()
    env={**os.environ,'CARGO_TARGET_DIR':str(cargo),'DISKTREE_VERSION':version}
    nsh=os.environ.get('NSH')
    if not nsh:
        run('cargo','build','--manifest-path',str(source/'Cargo.toml'),'--release','--locked','-p','nsh-cli',env=env)
        nsh=str(cargo/'release/nsh')
    connection=ROOT/'connection.ts'
    original=connection.read_text() if connection.exists() else None
    try:
        connection.write_text((ROOT/'connection.example.ts').read_text())
        run(nsh,'check')
        run(nsh,'build')
    finally:
        if original is None:connection.unlink(missing_ok=True)
        else:connection.write_text(original)
    run('cargo','build','--manifest-path',str(ROOT/'native/Cargo.toml'),'--release','--locked',env=env)
    run(sys.executable,'-m','PyInstaller','--noconfirm','--clean','--onedir','--name','disktree-scanner','--distpath',str(build/'scanner-dist'),'--workpath',str(build/'scanner-work'),'--specpath',str(build),str(ROOT/'scripts/service.py'))
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
    shutil.copy2(ROOT/'out/bundle.js',resources/'bundle.js')
    shutil.copytree(build/'scanner-dist/disktree-scanner',resources/'scanner')
    shutil.copy2(ROOT/'LICENSE',stage/'LICENSE')
    shutil.copy2(source/'LICENSE-APACHE',stage/'LICENSE.nsh')
    if system=='Darwin':run('codesign','--force','--deep','--sign','-','--timestamp=none',str(stage/'Disktree.app'))
    # Validate both the CLI wrapper and the embedded scanner, without opening a window.
    run(str(stage/'disktree'),'--version')
    scanner=subprocess.Popen([str(resources/'scanner/disktree-scanner'),'--packaged'],stdout=subprocess.PIPE,text=True,env={**os.environ,'DISKTREE_STATE':str(build/'scanner-smoke-state.json')})
    try:
        import json,urllib.request
        config=json.loads(scanner.stdout.readline())
        request=urllib.request.Request(config['endpoint']+'/scan',data=json.dumps({'path':str(stage)}).encode(),headers={'Authorization':'Bearer '+config['token'],'Content-Type':'application/json'})
        with urllib.request.urlopen(request,timeout=30) as response:assert json.load(response)['root']['files']>0
    finally:scanner.terminate();scanner.wait(timeout=10)
    dist=ROOT/'dist';dist.mkdir(exist_ok=True)
    archive=dist/f'disktree-v{version}-{target}.tar.gz'
    with tarfile.open(archive,'w:gz') as tar:
        for child in sorted(stage.iterdir()):tar.add(child,arcname=child.name)
    print('Built',archive)
    print('SHA256',hashlib.sha256(archive.read_bytes()).hexdigest())
if __name__=='__main__':main()
