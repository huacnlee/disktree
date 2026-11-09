#!/usr/bin/env python3
"""Build an installable, self-contained nsh app for the current platform."""
import argparse, json, hashlib, os, platform, plistlib, shutil, subprocess, sys, tarfile
from pathlib import Path

ROOT=Path(__file__).resolve().parent.parent
def run(*command,**kwargs):
    subprocess.run(command,cwd=ROOT,check=True,**kwargs)
def archive_names(version,target):
    # Keep the established installer name and target. The longer updater
    # suffix lets release-asset matchers prefer the normal installer.
    stem=f'disktree-v{version}-{target}'
    return f'{stem}.tar.gz',f'{stem}-updater.tar.gz'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--version',default='0.1.6')
    parser.add_argument('--target',default=None)
    args=parser.parse_args()
    version=args.version.removeprefix('v')
    system=platform.system()
    if system not in ('Darwin','Linux'):raise SystemExit('Build on macOS or Linux')
    default_arch='aarch64' if platform.machine() in ('arm64','aarch64') else 'x86_64'
    target=args.target or f"{default_arch}-{'apple-darwin' if system=='Darwin' else 'unknown-linux-gnu'}"
    arch=target.split('-',1)[0]
    build=ROOT/'.nsh/package'
    build.mkdir(parents=True,exist_ok=True)
    env={**os.environ,'DISKTREE_VERSION':version}
    metadata=json.loads(subprocess.check_output(['cargo','metadata','--format-version','1','--locked'],cwd=ROOT,env=env,text=True))
    cargo=Path(metadata['target_directory'])
    cargo_cmd=['cargo','build','--manifest-path',str(ROOT/'Cargo.toml'),'--release','--locked','--no-default-features']
    if args.target:cargo_cmd.extend(['--target',target])
    run(*cargo_cmd,env=env)
    binary=(cargo/target/'release/disktree') if args.target else (cargo/'release/disktree')
    stage=build/f'disktree-{version}-{target}'
    if stage.exists():shutil.rmtree(stage)
    stage.mkdir()
    if system=='Darwin':
        app=stage/'Disktree.app/Contents'
        (app/'MacOS').mkdir(parents=True)
        resources=app/'Resources';resources.mkdir()
        shutil.copy2(binary,app/'MacOS/disktree')
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
        shutil.copy2(binary,stage/'disktree')
        (stage/'disktree.desktop').write_text('[Desktop Entry]\nType=Application\nName=Disktree\nComment=Explore disk space with a native treemap\nExec=disktree\nTerminal=false\nCategories=System;Utility;\nIcon=disktree\n')
    # Native Rust apps call nsh-updater directly. Keep the same package layout
    # and configuration identity checks as NSH-generated hosts.
    config=json.loads((ROOT/'nsh.json').read_text())
    nsh=resources/'nsh' if system=='Darwin' else stage/'nsh'
    (nsh/'assets').mkdir(parents=True,exist_ok=True)
    (nsh/'app.json').write_text(json.dumps({'id':config['id'],'version':version}))
    updater={**config['updater'],'id':config['id'],'version':version}
    (nsh/'assets/nsh-updater.json').write_text(json.dumps(updater))
    (stage/'.disktree-install').write_text(f'v{version} {target}\n')
    shutil.copy2(ROOT/'assets/icon.svg',resources/'icon.svg')
    shutil.copy2(ROOT/'LICENSE',stage/'LICENSE')
    shutil.copy2(ROOT/'assets/LICENSE.nsh',stage/'LICENSE.nsh')
    if system=='Darwin':run('codesign','--force','--deep','--sign','-','--timestamp=none',str(stage/'Disktree.app'))
    # Validate the CLI wrapper before archiving when running on a compatible host.
    if not args.target or arch==default_arch:
        run(str(stage/'disktree'),'--version')
    dist=ROOT/'.nsh/dist';dist.mkdir(exist_ok=True)
    installer_name,updater_name=archive_names(version,target)
    archive=dist/installer_name
    with tarfile.open(archive,'w:gz') as tar:
        for child in sorted(stage.iterdir()):tar.add(child,arcname=child.name)
    # Keep the existing flat installer archive compatible. The updater expects
    # one top-level installation directory (or one .app on macOS).
    os_key='macos' if system=='Darwin' else 'linux'
    update_archive=dist/updater_name
    with tarfile.open(update_archive,'w:gz') as tar:
        if system=='Darwin':tar.add(stage/'Disktree.app',arcname='Disktree.app')
        else:tar.add(stage,arcname='disktree')
    manifest={'id':config['id'],'version':version,'platforms':{f'{os_key}-{arch}':{
        'url':update_archive.name,'sha256':hashlib.sha256(update_archive.read_bytes()).hexdigest(),
        'size':update_archive.stat().st_size}}}
    (dist/f'latest-{os_key}-{arch}.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print('Built updater archive',update_archive)
    print('Built',archive)
    print('SHA256',hashlib.sha256(archive.read_bytes()).hexdigest())
if __name__=='__main__':main()
