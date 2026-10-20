"""Launch the actual packaged native app and reject runtime script failures."""
import os, subprocess, tempfile, json
from pathlib import Path
root=Path(__file__).resolve().parent.parent
packages=list((root/'.nsh/package').glob('disktree-*/disktree'))
if len(packages)!=1:raise SystemExit(f'Expected one staged package, found {len(packages)}')
scratch=root/'.nsh/test-data'
scratch.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(dir=scratch) as temporary:
    environment={**os.environ,'DISKTREE_ROOT':str(root/'tests'),'DISKTREE_STATE':str(Path(temporary)/'state.json'),'DISKTREE_UI_SCRIPT':os.environ.get('DISKTREE_UI_SCRIPT','wait 5000; quit')}
    # Sidecars inherit stderr. A file avoids waiting on inherited pipe handles
    # after the native GUI process has already exited.
    with (Path(temporary)/'app.log').open('w+') as log:
        process=subprocess.Popen([str(packages[0])],env=environment,stdout=log,stderr=subprocess.STDOUT)
        print('Smoke app PID:',process.pid,flush=True)
        try:code=process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill();process.wait();log.seek(0);print(log.read());raise
        log.seek(0);output=log.read();print(output)
        if code or any(s in output.lower() for s in ['uncaught','out of memory','exceeded its budget','panicked at']):
            raise SystemExit('Packaged app smoke test failed')
    state=Path(temporary)/'state.json'
    if not state.is_file() or json.loads(state.read_text()).get('directory')!=str(root/'tests'):
        raise SystemExit('Packaged app did not complete its automatic scan through the Rust module')
print('Packaged native app smoke test passed (in-process Rust scanner completed startup scan)')
