import * as development from './connection';
interface Connection { endpoint: string; token: string; initialPath: string }
const packaged = (globalThis as unknown as { __DISKTREE_CONNECTION__?: Connection }).__DISKTREE_CONNECTION__;
export const endpoint = packaged?.endpoint ?? development.endpoint;
export const token = packaged?.token ?? development.token;
export const initialPath = packaged?.initialPath ?? development.initialPath;
