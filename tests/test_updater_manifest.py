"""Release manifests must match all platform archives before publishing."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('update_manifest', ROOT / 'scripts/update-manifest.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ManifestTests(unittest.TestCase):
    def test_installer_names_stay_compatible_and_updaters_are_distinct(self):
        spec = importlib.util.spec_from_file_location('build_release', ROOT / 'scripts/build-release.py')
        build = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(build)
        installer, updater = build.archive_names('1.2.3', 'x86_64-unknown-linux-gnu')
        self.assertEqual(installer, 'disktree-v1.2.3-x86_64-unknown-linux-gnu.tar.gz')
        self.assertEqual(updater, 'disktree-v1.2.3-x86_64-unknown-linux-gnu-updater.tar.gz')
        self.assertGreater(len(updater), len(installer))

    def fragment(self, directory, platform, version='1.0.0'):
        name = f'disktree-{version}-{platform}.tar.gz'
        archive = directory / name
        archive.write_bytes(platform.encode())
        fragment = {'id': 'com.huacnlee.disktree', 'version': version, 'platforms': {platform: {
            'url': name, 'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'size': archive.stat().st_size,
        }}}
        (directory / f'latest-{platform}.json').write_text(json.dumps(fragment))
        return archive

    def test_all_platforms_are_merged(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.fragment(directory, 'linux-x86_64')
            self.fragment(directory, 'macos-aarch64')
            self.fragment(directory, 'macos-x86_64')
            manifest = module.merge(directory)
            self.assertEqual(set(manifest['platforms']), {'linux-x86_64', 'macos-aarch64', 'macos-x86_64'})
            self.assertEqual(json.loads((directory / 'latest.json').read_text()), manifest)

    def test_mixed_versions_and_changed_archives_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.fragment(directory, 'linux-x86_64')
            self.fragment(directory, 'macos-aarch64', version='2.0.0')
            with self.assertRaisesRegex(ValueError, 'same app and version'):
                module.merge(directory)
            self.fragment(directory, 'macos-aarch64').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'does not match'):
                module.merge(directory)
