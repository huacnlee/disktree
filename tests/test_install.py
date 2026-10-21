import hashlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class InstallerTests(unittest.TestCase):
    def fixture(self, temporary, system):
        base = Path(temporary).resolve()
        fixtures = base / 'fixtures'
        fixtures.mkdir()
        tools = base / 'tools'
        tools.mkdir()
        (tools / 'uname').write_text(f'#!/bin/sh\ncase "$1" in -s) echo {system};; -m) echo arm64;; esac\n')
        (tools / 'curl').write_text(f'''#!{sys.executable}
import os, sys, shutil
from pathlib import Path
a = sys.argv[1:]
out = a[a.index('-o') + 1]
url = next(x for x in a if x.startswith('https://'))
if '-w' in a:
    print('https://github.com/huacnlee/disktree/releases/tag/v0.1.0', end='')
else:
    shutil.copyfile(Path(os.environ['FIXTURES']) / url.rsplit('/', 1)[1], out)
''')
        for tool in tools.iterdir():
            tool.chmod(0o755)
        env = {key: value for key, value in os.environ.items()
               if not key.startswith('DISKTREE_') and key not in ('XDG_DATA_HOME', 'XDG_STATE_HOME')}
        env.update(HOME=str(base / 'home'), FIXTURES=str(fixtures),
                   XDG_DATA_HOME=str(base / 'home/data'), PATH=str(tools) + ':' + os.environ['PATH'])
        return base, env

    def package(self, base, system, version='v0.1.0', executable=True):
        target = 'apple-darwin' if system == 'Darwin' else 'unknown-linux-gnu'
        archive = base / 'fixtures' / f'disktree-{version}-aarch64-{target}.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            files = {'LICENSE': b'fixture license\n'}
            body = f'#!/bin/sh\necho disktree {version[1:]}\n'.encode()
            if executable:
                files['disktree'] = body
            if system == 'Darwin':
                files['Disktree.app/Contents/MacOS/disktree'] = body
            else:
                files['disktree.desktop'] = b'[Desktop Entry]\nExec=disktree\n'
                files['resources/icon.svg'] = b'<svg/>\n'
            for name, data in files.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755 if name.endswith('disktree') else 0o644
                tar.addfile(info, io.BytesIO(data))
        (base / 'fixtures/SHA256SUMS').write_text(
            hashlib.sha256(archive.read_bytes()).hexdigest() + '  ' + archive.name + '\n')
        return archive

    def install(self, env, version='v0.1.0'):
        return subprocess.run(['sh', str(ROOT / 'install.sh'), version], env=env, capture_output=True, text=True)

    def versions(self, root):
        return sorted(path for path in root.glob('v*') if path.is_dir() and not path.is_symlink())

    def assert_active(self, base, system, version):
        executable = base / 'home/.local/bin/disktree'
        self.assertEqual(subprocess.check_output([str(executable)], text=True).strip(), f'disktree {version[1:]}')
        release = executable.resolve().parent
        self.assertEqual((release / '.disktree-install').read_text().split()[0], version)
        if system == 'Darwin':
            self.assertEqual((base / 'home/Applications/Disktree.app').resolve(), release / 'Disktree.app')
        else:
            self.assertIn(str(release / 'disktree'), (base / 'home/data/applications/disktree.desktop').read_text())
        return release

    def test_verified_release_installs_and_checksum_mismatch_is_rejected(self):
        for system in ['Darwin', 'Linux']:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temporary:
                base, env = self.fixture(temporary, system)
                self.package(base, system)
                success = self.install(env)
                self.assertEqual(success.returncode, 0, success.stderr)
                active = self.assert_active(base, system, 'v0.1.0')
                self.package(base, system, 'v0.1.1')
                checksum = base / 'fixtures/SHA256SUMS'
                checksum.write_text('0' * 64 + '  ' + checksum.read_text().split()[1] + '\n')
                failure = self.install(env, 'v0.1.1')
                self.assertNotEqual(failure.returncode, 0)
                self.assertIn('Checksum verification failed', failure.stderr)
                self.assertEqual(self.assert_active(base, system, 'v0.1.0'), active)
                self.assertEqual(self.versions(active), [])

    def test_upgrade_and_reinstall_keep_only_current_version(self):
        for system in ['Darwin', 'Linux']:
            with self.subTest(system=system), tempfile.TemporaryDirectory(prefix='disktree spaces ') as temporary:
                base, env = self.fixture(temporary, system)
                for version in ['v0.1.0', 'v0.1.1', 'v0.1.1']:
                    self.package(base, system, version)
                    result = self.install(env, version)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    active = self.assert_active(base, system, version)
                    self.assertEqual(active, base / 'home/data/disktree')
                    self.assertEqual(self.versions(active), [])
                    self.assertEqual(sorted(p.name for p in active.parent.iterdir() if p.name.startswith('.disktree')), [])

    def test_default_install_migrates_legacy_versions_and_preserves_unrelated_data(self):
        for system in ['Darwin', 'Linux']:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temporary:
                base, env = self.fixture(temporary, system)
                legacy = base / 'home/data/nsh-disktree'
                old = legacy / 'v0.1.0-aarch64-apple-darwin.abcdef'
                old.mkdir(parents=True)
                (old / 'disktree').write_text('old release')
                # Partial extractions are also installer-owned historical versions.
                (legacy / 'v0.1.1-aarch64-unknown-linux-gnu.ghijkl').mkdir()
                (legacy / 'notes.txt').write_text('keep me')
                outside = base / 'unrelated'
                outside.mkdir()
                (outside / 'sentinel').write_text('keep me too')
                (legacy / 'v0.0.1-aarch64-apple-darwin.mnopqr').symlink_to(outside, target_is_directory=True)
                self.package(base, system, 'v0.1.1')
                result = self.install(env, 'v0.1.1')
                self.assertEqual(result.returncode, 0, result.stderr)
                active = self.assert_active(base, system, 'v0.1.1')
                self.assertEqual(active, base / 'home/data/disktree')
                self.assertEqual(self.versions(legacy), [])
                self.assertEqual((legacy / 'notes.txt').read_text(), 'keep me')
                self.assertEqual((outside / 'sentinel').read_text(), 'keep me too')

    def test_custom_install_root_is_cleaned_without_touching_legacy_default(self):
        with tempfile.TemporaryDirectory() as temporary:
            base, env = self.fixture(temporary, 'Darwin')
            root = base / 'custom install'
            env['DISKTREE_INSTALL_DIR'] = str(root)
            legacy = base / 'home/data/nsh-disktree/v0.1.0-aarch64-apple-darwin.abcdef'
            legacy.mkdir(parents=True)
            for version in ['v0.1.0', 'v0.1.1']:
                self.package(base, 'Darwin', version)
                result = self.install(env, version)
                self.assertEqual(result.returncode, 0, result.stderr)
                active = self.assert_active(base, 'Darwin', version)
                self.assertEqual(active, root)
                self.assertEqual(self.versions(root), [])
            self.assertTrue(legacy.exists())

    def test_versioned_install_root_is_flattened(self):
        for system in ['Darwin', 'Linux']:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temporary:
                base, env = self.fixture(temporary, system)
                root = base / 'home/data/disktree'
                for name in ['v0.1.0-aarch64-apple-darwin.abcdef', 'v0.1.1-aarch64-unknown-linux-gnu.ghijkl']:
                    (root / name).mkdir(parents=True)
                    (root / name / 'disktree').write_text('old release')
                self.package(base, system, 'v0.1.2')
                result = self.install(env, 'v0.1.2')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.assert_active(base, system, 'v0.1.2'), root)
                self.assertEqual(self.versions(root), [])

    def test_unrelated_install_root_is_not_replaced(self):
        with tempfile.TemporaryDirectory() as temporary:
            base, env = self.fixture(temporary, 'Linux')
            root = base / 'mine'
            root.mkdir()
            (root / 'notes.txt').write_text('keep me')
            env['DISKTREE_INSTALL_DIR'] = str(root)
            self.package(base, 'Linux')
            result = self.install(env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('refusing to replace it', result.stderr)
            self.assertEqual((root / 'notes.txt').read_text(), 'keep me')
            self.assertEqual(sorted(p.name for p in base.iterdir() if p.name.startswith('.disktree')), [])

    def test_invalid_release_removes_partial_extraction_and_preserves_active_version(self):
        for system in ['Darwin', 'Linux']:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temporary:
                base, env = self.fixture(temporary, system)
                self.package(base, system)
                self.assertEqual(self.install(env).returncode, 0)
                active = self.assert_active(base, system, 'v0.1.0')
                self.package(base, system, 'v0.1.1', executable=False)
                result = self.install(env, 'v0.1.1')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Release has no disktree executable', result.stderr)
                self.assertEqual(self.assert_active(base, system, 'v0.1.0'), active)
                self.assertEqual(sorted(p.name for p in active.parent.iterdir() if p.name.startswith('.disktree')), [])


if __name__ == '__main__':
    unittest.main()
