"""Launch the actual packaged native app and reject runtime script failures."""
import os, subprocess, tempfile
from pathlib import Path
root=Path(__file__).resolve().parent.parent
packages=list((root/'.build').glob('disktree-*/disktree'))
if len(packages)!=1:raise SystemExit(f'Expected one staged package, found {len(packages)}')
with tempfile.TemporaryDirectory() as temporary:
    environment={**os.environ,'DISKTREE_ROOT':str(root/'tests'),'DISKTREE_STATE':str(Path(temporary)/'state.json'),'DISKTREE_UI_SCRIPT':os.environ.get('DISKTREE_UI_SCRIPT','wait 3000; quit')}
    result=subprocess.run([str(packages[0])],env=environment,capture_output=True,text=True,timeout=30)
    print(result.stdout+result.stderr)
    if result.returncode or any(s in result.stderr.lower() for s in ['uncaught','out of memory','exceeded its budget','panicked at']):
        raise SystemExit('Packaged app smoke test failed')
print('Packaged native app smoke test passed')
