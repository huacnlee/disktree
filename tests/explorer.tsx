import { mount } from "nsh";
import { App } from "../src/App";
import type { DiskNode, Scanner } from "../src/disk";

function node(name: string, size: number, directory = false): DiskNode {
  return {
    name,
    path: "/Volumes/Workspace/" + name,
    directory,
    size,
    apparent: size,
    files: 1,
    modified: 1700000000,
    children: [],
  };
}
const root = {
  ...node("Workspace", 3000000, true),
  path: "/Volumes/Workspace",
  files: 3,
};
const folder = node("Projects", 1500000, true);
const nested = { ...node("main.ts", 1500000), path: folder.path + "/main.ts" };
folder.children = [nested];
let items = [folder, node("recording.mov", 950000), node("notes.pdf", 550000)];
const snapshot = () => ({
  id: "fixture",
  root,
  total: 6000000,
  free: 3000000,
  errors: [],
});
const scanner: Scanner = {
  scan: async () => {
    await new Promise((resolve) => setTimeout(resolve, 350));
    return snapshot();
  },
  scanProgress: async () => ({
    path: root.path,
    percent: 42,
    directories: 1,
    files: 2,
    bytes: 2450000,
    errors: 0,
  }),
  cancelScan: async () => true,
  removalProgress: async () => null,
  view: async (args) => {
    const parent = args.path === folder.path ? folder : root;
    const children = (parent === folder ? [nested] : items).filter((n) =>
      n.name.includes(args.query),
    );
    return {
      node: parent,
      children,
      map: children,
      ancestors: parent === folder ? [root, folder] : [root],
      offset: 0,
      count: children.length,
      pageSize: 80,
    };
  },
  remove: async (mode, id, paths) => {
    if (mode !== "delete" || id !== "fixture")
      throw Error("Unexpected removal");
    items = items.filter((n) => !paths.includes(n.path));
    return { completed: paths, snapshot: snapshot() };
  },
};
mount(() => <App scanner={scanner} />);
