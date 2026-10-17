import { mount, Text } from 'nsh';
import { kind, type DiskNode } from '../disk';
function Test() {
  const cases = [
    ['/Users/jason/work/project/.git/objects','Git'],
    ['/Users/jason/work/project/target-gallery','Builds'],
    ['/Users/jason/work/pkg/mod','Caches'],
    ['/Users/jason/work/project/cache','Caches'],
    ['/Users/jason/work/project/src/main.ts','Code'],
    ['/Users/jason/work/project/photo.jpg','Media'],
    ['/Users/jason/.rustup/toolchains','Toolchains'],
    ['/Users/jason/.codex/sessions','Agent data'],
    ['/Users/jason/Dropbox/notes','Synced'],
    ['/Users/jason/Documents/letter.pdf','Documents'],
  ];
  for (const [path,label] of cases) {
    const actual=kind({path} as DiskNode);
    if(actual.label!==label) throw new Error(`${path}: expected ${label}, got ${actual.label}`);
  }
  return <Text>Classification regression checks passed</Text>;
}
mount(Test);
