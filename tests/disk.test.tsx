import { expect, test } from "nsh/test";
import { kind, mosaic, sizeStops, sizeColor, type DiskNode } from "../src/disk";
test("file categories prefer specific path segments", () => {
  const cases = [
    ["/Users/jason/work/project/.git/objects", "Git"],
    ["/Users/jason/work/project/target-gallery", "Builds"],
    ["/Users/jason/work/pkg/mod", "Caches"],
    ["/Users/jason/work/project/cache", "Caches"],
    ["/Users/jason/work/project/src/main.ts", "Code"],
    ["/Users/jason/work/project/photo.jpg", "Media"],
    ["/Users/jason/.rustup/toolchains", "Toolchains"],
    ["/Users/jason/.codex/sessions", "Agent data"],
    ["/Users/jason/Dropbox/notes", "Synced"],
    ["/Users/jason/Documents/letter.pdf", "Documents"],
  ];
  for (const [path, label] of cases) {
    const actual = kind({ path } as DiskNode);
    expect(actual.label).toBe(label);
  }
});
test("treemap partitions its measured area and uses the disk-size color scale", () => {
  const data = {
    path: "/fixture",
    apparent: 3,
    files: 1,
    directory: false,
    modified: 0,
    children: [],
  };
  const nodes: DiskNode[] = [
    { ...data, name: "a", size: 3 },
    { ...data, name: "b", size: 1 },
  ];
  const tiles = mosaic(nodes, 976, 680, "size", 2);
  expect(tiles.map((t) => [t.x, t.y, t.width, t.height])).toEqual([
    [0, 0, 732, 680],
    [732, 0, 244, 680],
  ]);
  for (const stop of sizeStops) expect(sizeColor(stop.bytes)).toBe(stop.color);
});
