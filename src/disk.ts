export interface DiskNode {
  path: string; name: string; directory: boolean; aggregate?: boolean;
  size: number; apparent: number; files: number; modified: number; children: DiskNode[];
}
export interface Snapshot { id: string; root: DiskNode; total: number; free: number; errors: string[]; errorCount?: number }
export interface DirectoryView { node: DiskNode; children: DiskNode[]; map: DiskNode[]; ancestors: DiskNode[]; offset: number; count: number; pageSize: number }
export type Metric = 'size' | 'apparent' | 'files';
export interface Tile { node: DiskNode; x: number; y: number; width: number; height: number; depth: number }
export function formatSize(value: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (value >= 1024 && i < 4) { value /= 1024; i++; }
  return `${value.toFixed(i ? 1 : 0)} ${units[i]}`;
}
export const categories = [
  {label:'Code',color:'#477de0'}, {label:'Git',color:'#e65883'},
  {label:'Caches',color:'#e9b64d'}, {label:'Builds',color:'#43cda1'},
  {label:'Toolchains',color:'#9ecb4e'}, {label:'Agent data',color:'#e38b51'},
  {label:'Media',color:'#aa6ee5'}, {label:'Documents',color:'#72a4b6'},
  {label:'Synced',color:'#42b9c6'}, {label:'Other',color:'#626d81'},
];
export function kind(node: DiskNode): { label: string; color: string } {
  const path = node.path.toLowerCase();
  // Specific descendants take precedence over broad ancestors such as /work.
  if (/(?:^|\/)\.git(?:\/|$)/.test(path)) return categories[1];
  if (/(?:^|\/)(?:\.?cache[s]?|node_modules|pkg\/mod)(?:\/|$)/.test(path)) return categories[2];
  if (/(?:^|\/)(?:target(?:-[^/]+)?|build|dist|deriveddata)(?:\/|$)/.test(path)) return categories[3];
  if (/(?:^|\/)(?:\.cargo|\.rustup|toolchains|coresimulator|xcode|android|\.android|\.bun|\.npm)(?:\/|$)/.test(path)) return categories[4];
  if (/(?:^|\/)(?:\.codex|\.claude|\.cursor|sessions)(?:\/|$)/.test(path)) return categories[5];
  if (/\.(?:mp4|mov|mp3|wav|jpg|jpeg|png|heic|webp|gif)$|\/(?:pictures|movies|music)(?:\/|$)/.test(path)) return categories[6];
  if (/(?:^|\/)(?:dropbox|onedrive|icloud|mobile documents)(?:\/|$)/.test(path)) return categories[8];
  if (/\.(?:pdf|docx?|txt|md|pages|xlsx?)$|\/documents(?:\/|$)/.test(path)) return categories[7];
  if (/\.(?:tsx?|jsx?|rs|py|go|rb|swift)$|\/(?:src|code|work|github)(?:\/|$)/.test(path)) return categories[0];
  return categories[9];
}
/** A stable tint distinguishes sibling projects without inventing categories. */
export function tileColor(node: DiskNode, depth: number): string {
  const base=kind(node).color;
  let hash=0; for(const char of node.name)hash=(hash*31+char.charCodeAt(0))|0;
  const shift=(Math.abs(hash)%19)-9+(depth-1)*5;
  return '#'+[1,3,5].map(i=>Math.max(0,Math.min(255,parseInt(base.slice(i,i+2),16)+shift)).toString(16).padStart(2,'0')).join('');
}
// Balanced binary subdivision preserves area exactly without stretching small siblings into strips.
export function mosaic(nodes: DiskNode[], width: number, height: number, metric: Metric, levels: number, zoom = 1): Tile[] {
  const result: Tile[] = [];
  function split(items: DiskNode[], x: number, y: number, w: number, h: number, depth: number) {
    if (!items.length || w * zoom < 2 || h * zoom < 2) return;
    if (items.length === 1) {
      const node = items[0];
      result.push({ node, x, y, width: w, height: h, depth });
      if (depth < levels && node.children.length && w * zoom > 90 && h * zoom > 65)
        split(ordered(node.children), x + 3 / zoom, y + 25 / zoom, w - 6 / zoom, h - 28 / zoom, depth + 1);
      return;
    }
    const total = items.reduce((sum,n) => sum+n[metric],0);
    let sum = 0, index = 0;
    while (index < items.length - 1 && sum < total / 2) sum += items[index++][metric];
    const ratio = sum / total;
    if (w >= h) {
      split(items.slice(0,index),x,y,w*ratio,h,depth);
      split(items.slice(index),x+w*ratio,y,w*(1-ratio),h,depth);
    } else {
      split(items.slice(0,index),x,y,w,h*ratio,depth);
      split(items.slice(index),x,y+h*ratio,w,h*(1-ratio),depth);
    }
  }
  function ordered(items:DiskNode[]) { return items.filter(n=>n[metric]>0).sort((a,b)=>b[metric]-a[metric]); }
  split(ordered(nodes),0,0,width,height,1);
  return result;
}
