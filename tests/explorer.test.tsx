import { expect, open, test } from "nsh/test";

test("browse, filter, and review removals before applying them", async () => {
  const app = await open("./explorer.tsx", { width: 1280, height: 850 });
  await app.waitFor("label:Select Projects");
  await app.press("label:Select Projects");
  await app.press("label:Open folder");
  await app.waitFor("label:Select main.ts");
  await app.press("label:Parent folder");
  await app.waitFor("label:Select notes.pdf");
  await app.press("label:View options");
  await app.press("label:Empty Trash…");
  await app.waitFor("label:Confirm empty Trash");
  await app.press("label:Back to explorer");
  await app.press("label:Select notes.pdf");
  await app.press("label:Mark");
  await app.press("label:Review marked items →");
  await app.press("label:Delete permanently");
  await app.waitFor("label:Confirm permanent deletion");
  await app.press("label:Back to explorer");
  await app.press("label:Back to contents");
  await app.waitFor("label:Select notes.pdf");
  await app.press("label:Review marked items →");
  await app.press("label:Delete permanently");
  await app.press("label:Confirm permanent deletion");
  await app.waitFor("label:Select recording.mov");
  expect(app.shows("label:Select notes.pdf")).toBe(false);
  await app.type("label:Filter names", "record");
  await app.waitUntil(() => !app.shows("label:Select Projects"));
  expect(app.shows("label:Select recording.mov")).toBe(true);
});

test("desktop shortcuts browse, mark and return to contents", async () => {
  const app = await open("./explorer.tsx", {
    width: 1280,
    height: 850,
    platform: "macos",
  });
  expect(app.window("main").state()).toMatchObject({
    visible: true,
    minimized: false,
  });
  await app.waitFor("label:Select Projects");
  for (const label of ["Files", "Apparent", "Disk size"]) {
    await app.press("label:" + label);
    await app.waitFor("label:Select Projects");
    expect(app.enabled("label:Scan folder")).toBe(true);
  }
  await app.keyboard.press("down");
  await app.waitFor("label:Back to contents");
  await app.keyboard.press("space");
  await app.waitFor("label:Unmark");
  await app.keyboard.press("enter");
  await app.waitFor("label:Select main.ts");
  await app.keyboard.press("alt+up");
  await app.waitFor("label:Select recording.mov");
  await app.press("label:recording.mov");
  await app.waitFor("label:Back to contents");
  await app.keyboard.press("escape");
  await app.waitFor("label:Select recording.mov");
});

test("rescan reports progress and restores the explorer", async () => {
  const app = await open("./explorer.tsx", {
    width: 1280,
    height: 850,
    clock: { time: 0 },
  });
  await app.waitFor("label:Select Projects");
  await app.press("label:Scan folder");
  await app.waitFor("text:Scanning folders · ~42%");
  await app.waitFor("id:scan-progress", { state: "gone" });
  await app.waitFor("label:Select Projects");
});

test("startup checks silently and the update menu opens the prompt on demand", async () => {
  const calls: string[] = [];
  const app = await open("./explorer.tsx", {
    width: 1280,
    height: 850,
    modules: {
      "nsh/updater": {
        checkForUpdate: () => {
          calls.push("check");
          return { version: "0.2.0" };
        },
        promptForUpdate: () => {
          calls.push("prompt");
        },
      },
    },
  });
  await app.waitFor("label:Update available");
  expect(calls).toEqual(["check"]);
  await app.press("label:View options");
  await app.press("label:Check for updates…");
  await app.waitUntil(() => calls.length === 2);
  expect(calls).toEqual(["check", "prompt"]);
});
