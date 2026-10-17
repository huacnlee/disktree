"""Loopback disk scanner used by the nsh desktop client."""
import json, os, secrets, shutil, stat, subprocess, sys, threading, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

TOKEN = secrets.token_urlsafe(32)
SNAPSHOTS = {}
LOCK = threading.Lock()
PAGE_SIZE = 80
MAP_BUDGET = 240

DEFAULT_STATE = Path.home() / 'Library/Application Support/Disktree/state.json' if sys.platform == 'darwin' else Path(os.environ.get('XDG_STATE_HOME', str(Path.home()/'.local/state'))) / 'nsh-disktree/state.json'
STATE_FILE = Path(os.environ.get('DISKTREE_STATE', str(DEFAULT_STATE)))

def saved_location(state_file=None):
    try:
        value = json.loads(Path(state_file or STATE_FILE).read_text()).get('directory', '')
        return value if os.path.isdir(value) else str(Path.home())
    except (OSError, ValueError, TypeError, AttributeError):
        return str(Path.home())

def save_location(directory, state_file=None):
    destination = Path(state_file or STATE_FILE)
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix('.tmp')
    with LOCK:
        temporary.write_text(json.dumps({'directory': directory}))
        os.replace(temporary, destination)

def scan(root):
    root = os.path.abspath(os.path.expanduser(root))
    if not os.path.isdir(root) or os.path.islink(root):
        raise ValueError('Choose a directory, not a symbolic link')
    seen = set()
    device = os.stat(root).st_dev
    errors = []
    records = {}
    nodes = {}
    def walk(path):
        info = os.lstat(path)
        directory = stat.S_ISDIR(info.st_mode)
        node = dict(path=path, name=os.path.basename(path) or '/', directory=directory,
                    size=0, apparent=0, files=0, modified=info.st_mtime, children=[])
        nodes[path] = node
        records[path] = (info.st_dev, info.st_ino, info.st_mtime_ns, info.st_size)
        if info.st_dev != device or stat.S_ISLNK(info.st_mode):
            return node
        if not directory:
            if not stat.S_ISREG(info.st_mode):
                return node
            key = (info.st_dev, info.st_ino)
            node['files'] = 1
            if key not in seen:
                seen.add(key)
                node['size'] = info.st_blocks * 512
                node['apparent'] = info.st_size
            return node
        # Avoid materializing cloud-only directories on macOS.
        if getattr(info, 'st_flags', 0) & 0x40000000:
            errors.append(path + ': cloud-only (skipped)')
            return node
        try:
            with os.scandir(path) as entries:
                for entry in entries:
                    try:
                        child = walk(entry.path)
                        node['children'].append(child)
                        for field in ('size', 'apparent', 'files'):
                            node[field] += child[field]
                    except OSError as error:
                        errors.append(entry.path + ': ' + str(error))
        except OSError as error:
            errors.append(path + ': ' + str(error))
        node['children'].sort(key=lambda n: (-n['size'], n['name']))
        return node
    tree = walk(root)
    usage = shutil.disk_usage(root)
    identity = uuid.uuid4().hex
    with LOCK:
        SNAPSHOTS[identity] = (root, records, nodes)
        while len(SNAPSHOTS) > 1:
            del SNAPSHOTS[next(iter(SNAPSHOTS))]
    return dict(id=identity, root=shallow(tree), total=usage.total, free=usage.free, errors=errors[:100], errorCount=len(errors))

def shallow(node):
    return {**node, 'children': []}

def view(identity, path, offset=0, query='', hidden=True, metric='size', levels=2):
    if metric not in ('size', 'apparent', 'files'):
        raise ValueError('Invalid metric')
    with LOCK:
        root, _, nodes = SNAPSHOTS[identity]
        node = nodes[path]
    if not node['directory']:
        raise ValueError('Choose a directory')
    children = sorted((n for n in node['children'] if
                       (hidden or not n['name'].startswith('.')) and query.lower() in n['name'].lower()),
                      key=lambda n: (-n[metric], n['name']))
    offset = max(0, min(int(offset), max(0, (len(children)-1)//PAGE_SIZE*PAGE_SIZE)))
    page = children[offset:offset+PAGE_SIZE]
    budget = [MAP_BUDGET]
    def bounded(items, depth):
        output = []
        for child in items:
            if budget[0] <= 1:
                break
            budget[0] -= 1
            item = shallow(child)
            output.append(item)
        # Reserve the summary before descending, so nested levels cannot exceed the budget.
        selected = len(output)
        rest = items[selected:]
        if rest:
            budget[0] -= 1
            output.append(dict(path=path+'/#remaining-'+str(depth)+'-'+str(budget[0]), name=f'{len(rest):,} more items',
                               directory=False, size=sum(n['size'] for n in rest), apparent=sum(n['apparent'] for n in rest),
                               files=sum(n['files'] for n in rest), modified=0, children=[], aggregate=True))
        for item, child in zip(output[:selected], items):
            if child['children'] and depth < min(3,max(1,int(levels))) and budget[0] > 1:
                item['children'] = bounded(child['children'], depth+1)
        return output
    tiles = bounded(page,1)
    rest = children[:offset]+children[offset+PAGE_SIZE:]
    if rest:
        tiles.append(dict(path=path+'/#other-pages', name=f'{len(rest):,} items on other pages', directory=False,
                          size=sum(n['size'] for n in rest), apparent=sum(n['apparent'] for n in rest),
                          files=sum(n['files'] for n in rest), modified=0, children=[], aggregate=True))
    ancestors = []
    cursor = path
    while True:
        ancestors.append(shallow(nodes[cursor]))
        if cursor == root:
            break
        cursor = os.path.dirname(cursor)
    return dict(node=shallow(node), children=[shallow(n) for n in page], map=tiles,
                ancestors=list(reversed(ancestors)), offset=offset, count=len(children), pageSize=PAGE_SIZE)

def trash(identity, paths):
    with LOCK:
        root, records, _ = SNAPSHOTS[identity]
    paths = list(dict.fromkeys(paths))
    if not paths:
        raise ValueError('Nothing marked')
    for path in paths:
        if path == root or path not in records or not path.startswith(root.rstrip('/') + '/'):
            raise ValueError('Only scanned descendants can be moved to Trash')
        if any(path.startswith(other.rstrip('/') + '/') for other in paths if path != other):
            raise ValueError('Overlapping selections')
        info = os.lstat(path)
        if (info.st_dev, info.st_ino, info.st_mtime_ns, info.st_size) != records[path]:
            raise ValueError('Selection changed since scan. Rescan before moving it.')
        # A replaced ancestor must never redirect a reviewed path.
        parent = os.path.dirname(path)
        while parent != os.path.dirname(root):
            current = os.lstat(parent)
            old = records.get(parent)
            if stat.S_ISLNK(current.st_mode) or not old or (current.st_dev, current.st_ino) != old[:2]:
                raise ValueError('Directory changed. Rescan first.')
            if parent == root:
                break
            parent = os.path.dirname(parent)
    # Validate every scanned descendant too: edits deep inside a marked directory
    # do not necessarily change the selected directory's own metadata.
    for path in paths:
        for descendant, old in records.items():
            if descendant.startswith(path.rstrip('/') + '/'):
                info = os.lstat(descendant)
                if (info.st_dev, info.st_ino, info.st_mtime_ns, info.st_size) != old:
                    raise ValueError('Contents changed since scan. Rescan before moving them.')
    if sys.platform != 'darwin' and not shutil.which('gio'):
        raise ValueError('Install gio (libglib2.0-bin) to use the system Trash')
    # Finder provides the standard recoverable Trash operation, including other volumes.
    completed = []
    for path in paths:
        try:
            command = ['osascript', '-e', 'on run argv\nset targetItem to POSIX file (item 1 of argv) as alias\ntell application "Finder" to delete targetItem\nend run', path] if sys.platform == 'darwin' else ['gio', 'trash', '--', path]
            subprocess.run(command, check=True, capture_output=True, text=True)
            completed.append(path)
        except subprocess.CalledProcessError as error:
            return dict(completed=completed, error=error.stderr.strip())
    return dict(completed=completed)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass
    def do_POST(self):
        if self.headers.get('Authorization') != 'Bearer ' + TOKEN:
            self.send_error(403)
            return
        try:
            data = json.loads(self.rfile.read(min(int(self.headers.get('Content-Length', 0)), 1048576)))
            if self.path == '/scan':
                result = scan(data.get('path') or saved_location())
                save_location(result['root']['path'])
            elif self.path == '/view':
                result = view(data['id'], data['path'], data.get('offset',0), data.get('query',''), data.get('hidden',True), data.get('metric','size'), data.get('levels',2))
                save_location(result['node']['path'])
            elif self.path == '/preferences':
                result = dict(directory=saved_location(), restore=STATE_FILE.is_file())
            elif self.path == '/trash':
                result = trash(data['id'], data['paths'])
            else:
                self.send_error(404)
                return
            status = 200
        except Exception as error:
            status, result = 400, dict(error=str(error))
        body = json.dumps(result).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

if __name__ == '__main__':
    if '--parent-pid' in sys.argv:
        parent_pid=int(sys.argv[sys.argv.index('--parent-pid')+1])
        def watch_parent():
            import time
            while True:
                time.sleep(1)
                try:os.kill(parent_pid,0)
                except ProcessLookupError:os._exit(0)
        threading.Thread(target=watch_parent,daemon=True).start()
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    connection = dict(endpoint='http://127.0.0.1:' + str(server.server_port), token=TOKEN,
                      initialPath=os.environ.get('DISKTREE_ROOT') or saved_location())
    if '--packaged' in sys.argv:
        print(json.dumps(connection), flush=True)
    else:
        config = Path(__file__).resolve().parent.parent / 'connection.ts'
        temporary = config.with_suffix('.ts.tmp')
        temporary.write_text(''.join('export const '+key+' = '+json.dumps(value)+';\n' for key,value in connection.items()))
        os.replace(temporary, config)
        print('Disk scanner ready', flush=True)
    server.serve_forever()
