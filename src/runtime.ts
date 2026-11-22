import { DiskTree } from "disktree-native";
import type { Scanner } from "./disk";

async function request<T>(route: string, data: unknown): Promise<T> {
  return JSON.parse(await DiskTree.request(route, JSON.stringify(data)));
}
export const scanner: Scanner = {
  scan: (path, progressToken) => request("/scan", { path, progressToken }),
  scanProgress: (progressToken) => request("/scan-progress", { progressToken }),
  cancelScan: (progressToken) => request("/cancel", { progressToken }),
  view: (params) => request("/view", params),
  remove: (mode, id, paths, progressToken) =>
    request(mode === "empty" ? "/empty-trash" : "/" + mode, {
      id,
      paths,
      progressToken,
    }),
  removalProgress: (progressToken) => request("/progress", { progressToken }),
  uiScript: () => request("/ui-script", {}),
  recordUpdate: (outcome, message, version) =>
    request("/smoke-record", { outcome, message, version }),
};
