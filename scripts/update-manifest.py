#!/usr/bin/env python3
"""Merge the per-platform update manifests produced by build-release.py."""
import argparse
import hashlib
import json
from pathlib import Path


def merge(directory):
    files = sorted(directory.glob('latest-*.json'))
    if not files:
        raise ValueError('No platform update manifests found')
    merged = None
    for path in files:
        fragment = json.loads(path.read_text())
        if merged is None:
            merged = {**fragment, 'platforms': {}}
        if any(fragment[key] != merged[key] for key in ('id', 'version')):
            raise ValueError('Platform manifests must describe the same app and version')
        for platform, asset in fragment['platforms'].items():
            if platform in merged['platforms']:
                raise ValueError(f'Duplicate platform: {platform}')
            if not (directory / asset['url']).is_file():
                raise ValueError(f'Missing update archive: {asset["url"]}')
            archive = directory / asset['url']
            if asset['size'] != archive.stat().st_size or asset['sha256'] != hashlib.sha256(archive.read_bytes()).hexdigest():
                raise ValueError(f'Update archive does not match manifest: {asset["url"]}')
            merged['platforms'][platform] = asset
    (directory / 'latest.json').write_text(json.dumps(merged, indent=2) + '\n')
    return merged


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    merge(parser.parse_args().directory)
