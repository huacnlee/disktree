"""Exercise the real nsh scan/render path with a large synthetic scan response."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import threading
import sys, os
from service import SNAPSHOTS, shallow, view

base = Path(__file__).resolve().parent.parent
count = int(sys.argv[1]) if len(sys.argv)>1 else 200000
leaf = dict(path='/fixture/archive/'+'long-directory-name/'*5+'file.bin',name='file.bin',directory=False,size=4096,apparent=4000,files=1,modified=1,children=[])
folder = dict(path='/fixture/archive',name='archive',directory=True,size=count*4096,apparent=count*4000,files=count,modified=1,children=[dict(leaf,path=leaf['path']+str(i),name='file-'+str(i)+'.bin') for i in range(count)])
root = dict(path='/fixture',name='fixture',directory=True,size=folder['size'],apparent=folder['apparent'],files=count,modified=1,children=[folder])
SNAPSHOTS['stress'] = ('/fixture', {}, {'/fixture': root, '/fixture/archive': folder})
payload_sizes = []
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_): pass
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))))
        if self.path == '/view':
            result = view(request['id'],request['path'],request.get('offset',0),request.get('query',''),request.get('hidden',True),request.get('metric','size'),request.get('levels',2))
        else:
            result = dict(id='stress',root=shallow(root),total=10**12,free=5*10**11,errors=[])
        body = json.dumps(result).encode()
        payload_sizes.append(len(body))
        self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers()
        try:self.wfile.write(body)
        except (BrokenPipeError,ConnectionResetError):pass
server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
connection=base/'connection.ts'
original=connection.read_text()
try:
    connection.write_text(f'export const initialPath="/fixture";\nexport const endpoint="http://127.0.0.1:{server.server_port}";\nexport const token="stress";\n')
    actions=['--tap','label:Scan folder','--tap','label:archive','--tap','label:Open folder']
    for _ in range(int(sys.argv[2]) if len(sys.argv)>2 else 2):
        for label in ['Next ›','Files','Next ›','Disk size','Next ›','‹ Previous']:
            actions += ['--tap','label:'+label]
    actions += ['--tap','label:↑ Parent']
    result=subprocess.run([os.environ.get('NSH','nsh'),'trace','--width','1200','--height','850',*actions,'--wait','500','--duration','1500'],cwd=base,capture_output=True,text=True,timeout=90)
    log=result.stdout+'\n'+result.stderr
    Path('/tmp/disktree-memory-stress.log').write_text(log)
    print(f'{count:,} files; largest response {max(payload_sizes, default=0)/1024:.1f} KiB; trace exit {result.returncode}')
    failure=next((line for line in log.splitlines() if any(s in line.lower() for s in ['out of memory','memory limit','allocation failed','exception','error:'])),None)
    print(failure or 'No memory error; paging and metric switching exercised')
    if result.returncode or failure or 'Scan complete' not in log or '81–160' not in log or max(payload_sizes)>1024*1024:sys.exit(1)
finally:
    connection.write_text(original)
    server.shutdown()
