import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from service import scan, trash, view, SNAPSHOTS, PAGE_SIZE, MAP_BUDGET, save_location, saved_location
import json

class ScannerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.folder = self.root / 'folder'
        self.folder.mkdir()
        self.file = self.folder / 'sample.bin'
        self.file.write_bytes(b'a' * 8192)
    def tearDown(self):
        self.temp.cleanup()
    def test_allocated_size_and_hardlinks(self):
        os.link(self.file, self.root / 'hardlink')
        os.symlink(self.folder, self.root / 'link')
        data = scan(str(self.root))
        self.assertEqual(data['root']['apparent'],8192)
        self.assertEqual(data['root']['size'],self.file.stat().st_blocks*512)
        self.assertEqual(data['root']['files'],2)
        self.assertGreater(data['total'],data['free'])
    def test_reject_scan_symlink(self):
        os.symlink(self.folder,self.root/'link')
        with self.assertRaises(ValueError): scan(str(self.root/'link'))
    def test_reject_root_and_outside(self):
        data=scan(str(self.root))
        for path in [str(self.root),'/etc/passwd']:
            with self.assertRaises(ValueError): trash(data['id'],[path])
    def test_reject_modified_file(self):
        data=scan(str(self.root))
        self.file.write_bytes(b'changed')
        with self.assertRaises(ValueError): trash(data['id'],[str(self.file)])
    def test_reject_modified_descendant(self):
        data=scan(str(self.root))
        self.file.write_bytes(b'changed')
        with self.assertRaises(ValueError): trash(data['id'],[str(self.folder)])
    def test_reject_replaced_ancestor(self):
        data=scan(str(self.root))
        self.folder.rename(self.root/'moved')
        os.symlink(self.root/'moved',self.folder)
        with self.assertRaises(ValueError): trash(data['id'],[str(self.file)])
    def test_reject_overlapping_marks(self):
        data=scan(str(self.root))
        with self.assertRaises(ValueError): trash(data['id'],[str(self.folder),str(self.file)])
    @patch('service.subprocess.run')
    def test_trash_passes_path_as_argument(self, run):
        data=scan(str(self.root))
        result=trash(data['id'],[str(self.file)])
        self.assertEqual(result['completed'],[str(self.file)])
        self.assertEqual(run.call_args.args[0][-1],str(self.file))

class DirectoryViewTests(unittest.TestCase):
    def setUp(self):
        def node(path, size, children=None):
            return dict(path=path,name=path.rsplit('/',1)[-1],directory=children is not None,
                        size=size,apparent=size,files=1,modified=1,children=children or [])
        self.children=[node('/fixture/file-'+str(i),i+1) for i in range(10000)]
        self.root=node('/fixture',sum(n['size'] for n in self.children),self.children)
        SNAPSHOTS['view-test']=('/fixture',{}, {'/fixture':self.root})
    def tearDown(self):
        SNAPSHOTS.pop('view-test',None)
    def test_payload_is_bounded_and_totals_are_preserved(self):
        data=view('view-test','/fixture')
        self.assertEqual(len(data['children']),PAGE_SIZE)
        self.assertEqual(data['node']['children'],[])
        self.assertEqual(data['count'],10000)
        self.assertEqual(sum(n['size'] for n in data['map']),self.root['size'])
        self.assertLess(len(json.dumps(data)),100000)
    def test_pages_do_not_drop_or_duplicate_items(self):
        paths=[]
        for offset in range(0,10000,PAGE_SIZE):
            data=view('view-test','/fixture',offset)
            paths.extend(n['path'] for n in data['children'])
        self.assertEqual(len(paths),10000)
        self.assertEqual(len(set(paths)),10000)
    def test_search_includes_items_outside_first_page(self):
        data=view('view-test','/fixture',query='file-0')
        self.assertEqual(data['count'],1)
        self.assertEqual(data['children'][0]['name'],'file-0')
    def test_nested_map_stays_within_node_budget(self):
        # A wide first child used to bring the entire scan into QuickJS.
        folder={**self.root,'path':'/fixture/archive','name':'archive'}
        self.root={**self.root,'children':[folder]}
        SNAPSHOTS['view-test']=('/fixture',{}, {'/fixture':self.root,'/fixture/archive':folder})
        data=view('view-test','/fixture',levels=3)
        def count(nodes):return sum(1+count(n['children']) for n in nodes)
        self.assertLessEqual(count(data['map']),MAP_BUDGET+1)
        self.assertEqual(sum(n['size'] for n in data['map'][0]['children']),folder['size'])

class LocationTests(unittest.TestCase):
    def test_directory_survives_a_new_read_and_can_be_updated(self):
        with tempfile.TemporaryDirectory() as directory:
            state=Path(directory)/'settings'/'state.json'
            first=Path(directory)/'first';first.mkdir()
            second=Path(directory)/'second';second.mkdir()
            save_location(str(first),state)
            self.assertEqual(saved_location(state),str(first))
            save_location(str(second),state)
            self.assertEqual(saved_location(state),str(second))
    def test_missing_or_corrupt_or_deleted_location_falls_back(self):
        with tempfile.TemporaryDirectory() as directory:
            state=Path(directory)/'state.json'
            self.assertEqual(saved_location(state),str(Path.home()))
            state.write_text('not json')
            self.assertEqual(saved_location(state),str(Path.home()))
            save_location(str(Path(directory)/'deleted'),state)
            self.assertEqual(saved_location(state),str(Path.home()))

if __name__=='__main__': unittest.main()
