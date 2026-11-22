#!/usr/bin/env python3
"""Build an installable, self-contained nsh app for the current platform."""
import argparse, json, hashlib, os, platform, plistlib, shutil, subprocess, tarfile, tempfile
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
    parser.add_argument('--version',default='0.1.1')
    args=parser.parse_args()
    version=args.version.removeprefix('v')
    system=platform.system()
    if system not in ('Darwin','Linux'):raise SystemExit('Build on macOS or Linux')
    arch='aarch64' if platform.machine() in ('arm64','aarch64') else 'x86_64'
    target=f"{arch}-{'apple-darwin' if system=='Darwin' else 'unknown-linux-gnu'}"
    build=ROOT/'.nsh/package'
    build.mkdir(parents=True,exist_ok=True)
    env={**os.environ,'DISKTREE_VERSION':version}
    config=json.loads((ROOT/'nsh.json').read_text())
    stage=build/f'disktree-{version}-{target}'
    if stage.exists():shutil.rmtree(stage)
    platform_name='macos' if system=='Darwin' else 'linux'
    with tempfile.TemporaryDirectory(dir=build) as temporary:
        output=Path(temporary)
        run('nsh','package','--target',platform_name,'--name','disktree',
            '--app-version',version,'--out',str(output),env=env)
        if system=='Darwin':
            stage.mkdir()
            shutil.move(output/'disktree.app',stage/'Disktree.app')
        else:
            shutil.move(output/'disktree',stage)
    if system=='Darwin':
        app=stage/'Disktree.app/Contents'
        resources=app/'Resources'
        with (app/'Info.plist').open('rb') as file:
            info=plistlib.load(file)
        info.update(CFBundleName='Disktree',CFBundleDisplayName='Disktree',CFBundleIconFile='AppIcon',CFBundleVersion=version)
        with (app/'Info.plist').open('wb') as file:
            plistlib.dump(info,file)
        run('sips','-s','format','png',str(ROOT/'assets/icon.svg'),'--out',str(build/'icon.png'))
        iconset=build/'AppIcon.iconset'
        if iconset.exists():shutil.rmtree(iconset)
        iconset.mkdir()
        for size in [16,32,128,256,512]:
            for scale in [1,2]:
                name=f'icon_{size}x{size}'+('@2x' if scale==2 else '')+'.png'
                run('sips','-z',str(size*scale),str(size*scale),str(build/'icon.png'),'--out',str(iconset/name))
        run('iconutil','-c','icns',str(iconset),'-o',str(resources/'AppIcon.icns'))
    else:
        (stage/'disktree').rename(stage/'disktree-host')
        resources=stage/'resources';resources.mkdir()
        (stage/f"{config['id']}.desktop").unlink()
        (stage/'disktree.desktop').write_text('[Desktop Entry]\nType=Application\nName=Disktree\nComment=Explore disk space with a native treemap\nExec=disktree\nTerminal=false\nCategories=System;Utility;\nIcon=disktree\n')
    launcher=stage/'disktree'
    launcher.write_text('''#!/bin/sh
set -eu
launcher=$0
while [ -L "$launcher" ]; do
  link=$(readlink "$launcher")
  case "$link" in /*) launcher=$link;; *) launcher=$(dirname "$launcher")/$link;; esac
done
root=$(CDPATH= cd -- "$(dirname -- "$launcher")" && pwd)
if [ -d "$root/Disktree.app" ]; then
  host="$root/Disktree.app/Contents/MacOS/disktree"
  if [ "${1:-}" = --version ]; then
    version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$root/Disktree.app/Contents/Info.plist")
    echo "disktree $version"
    exit 0
  fi
else
  host="$root/disktree-host"
  if [ "${1:-}" = --version ]; then
    read -r version target < "$root/.disktree-install"
    echo "disktree ${version#v}"
    exit 0
  fi
fi
exec "$host" "$@"
''')
    launcher.chmod(0o755)
    nsh=resources/'nsh' if system=='Darwin' else stage/'nsh'
    if not (nsh/'bundle.nshc').is_file():
        raise SystemExit('Packaged TSX bundle is missing')
    metadata=json.loads((nsh/'app.json').read_text())
    if metadata['id']!=config['id'] or metadata['version']!=version:
        raise SystemExit('Packaged app identity or version differs from the release')
    (nsh/'assets').mkdir(parents=True,exist_ok=True)
    updater={**config['updater'],'id':config['id'],'version':version}
    (nsh/'assets/nsh-updater.json').write_text(json.dumps(updater))
    (stage/'.disktree-install').write_text(f'v{version} {target}\n')
    shutil.copy2(ROOT/'assets/icon.svg',resources/'icon.svg')
    shutil.copy2(ROOT/'LICENSE',stage/'LICENSE')
    shutil.copy2(ROOT/'assets/LICENSE.nsh',stage/'LICENSE.nsh')
    if system=='Darwin':run('codesign','--force','--deep','--sign','-','--timestamp=none',str(stage/'Disktree.app'))
    reported=subprocess.check_output([stage/'disktree','--version'],text=True).strip()
    if reported!=f'disktree {version}':
        raise SystemExit(f'Unexpected launcher version: {reported}')
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
