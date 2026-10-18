import { Scanner } from 'disktree/native';
/** Filesystem work stays in Rust; JSX receives only bounded JSON results. */
export async function request(route:string,data:unknown) {
  return JSON.parse(await Scanner.request(route,JSON.stringify(data)));
}
