export interface Crumb { name:string; path:string }

/** Keep the root and nearest ancestors, reserving room for the overflow menu. */
export function collapseBreadcrumbs(crumbs:Crumb[], available:number) {
  const width=(crumb:Crumb)=>Math.min(180,Array.from(crumb.name).reduce((sum,c)=>sum+(c.codePointAt(0)!>255?12:7.3),0))+12;
  const total=crumbs.reduce((sum,crumb)=>sum+width(crumb),0)+Math.max(0,crumbs.length-1)*10;
  if(crumbs.length<3 || total<=available) return {root:crumbs[0],hidden:[],tail:crumbs.slice(1)};
  let start=crumbs.length-1;
  let used=width(crumbs[0])+40+width(crumbs[start])+10;
  while(start>2 && used+width(crumbs[start-1])+10<=available) {
    used+=width(crumbs[--start])+10;
  }
  return {root:crumbs[0],hidden:crumbs.slice(1,start),tail:crumbs.slice(start)};
}
