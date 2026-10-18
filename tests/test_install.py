import hashlib, io, os, platform, subprocess, sys, tarfile, tempfile, unittest
from pathlib import Path

class InstallerTests(unittest.TestCase):
    def test_verified_release_installs_and_checksum_mismatch_is_rejected(self):
        root=Path(__file__).resolve().parent.parent
        for system,target in [('Darwin','apple-darwin'),('Linux','unknown-linux-gnu')]:
            with self.subTest(system=system),tempfile.TemporaryDirectory() as temp:
                temp=Path(temp);fixture=temp/'fixtures';fixture.mkdir();bin_dir=temp/'tools';bin_dir.mkdir()
                archive=fixture/f'disktree-v0.1.0-aarch64-{target}.tar.gz'
                with tarfile.open(archive,'w:gz') as tar:
                    body=b'#!/bin/sh\necho disktree 0.1.0\n'
                    info=tarfile.TarInfo('disktree');info.size=len(body);info.mode=0o755
                    tar.addfile(info,io.BytesIO(body))
                checksums=fixture/'SHA256SUMS'
                checksums.write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
                (bin_dir/'uname').write_text(f'#!/bin/sh\ncase "$1" in -s) echo {system};; -m) echo arm64;; esac\n')
                (bin_dir/'curl').write_text(f'#!{sys.executable}\nimport os,sys,shutil\nfrom pathlib import Path\na=sys.argv[1:]\nout=a[a.index("-o")+1]\nurl=next(x for x in a if x.startswith("https://"))\nif "-w" in a: print("https://github.com/huacnlee/nsh-disktree/releases/tag/v0.1.0",end="")\nelse:shutil.copyfile(Path(os.environ["FIXTURES"])/url.rsplit("/",1)[1],out)\n')
                for tool in bin_dir.iterdir():tool.chmod(0o755)
                env={**os.environ,'HOME':str(temp/'home'),'FIXTURES':str(fixture),'PATH':str(bin_dir)+':'+os.environ['PATH'],'DISKTREE_BIN_DIR':str(temp/'home/bin'),'DISKTREE_INSTALL_DIR':str(temp/'home/data')}
                success=subprocess.run(['sh',str(root/'install.sh')],env=env,capture_output=True,text=True)
                self.assertEqual(success.returncode,0,success.stderr)
                exe=temp/'home/bin/disktree'
                self.assertEqual(subprocess.check_output([str(exe)],text=True).strip(),'disktree 0.1.0')
                checksums.write_text('0'*64+'  '+archive.name+'\n')
                failure=subprocess.run(['sh',str(root/'install.sh'),'v0.1.0'],env=env,capture_output=True,text=True)
                self.assertNotEqual(failure.returncode,0)
                self.assertIn('Checksum verification failed',failure.stderr)
if __name__=='__main__':unittest.main()
