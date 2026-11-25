import {
  signal,
  Animated,
  createAnimatedValue,
  LayoutAnimation,
  reduceMotion,
  keymap,
  computed,
  effect,
  cleanup,
  batch,
  View,
  Text,
  Scroll,
  type ViewRef,
} from "nsh";
import { Input, Menu as OptionsMenu } from "nsh/base";

import {
  Menu,
  isAvailable as desktopAvailable,
  Window,
  currentWindow,
  titleBar,
} from "nsh/desktop";

import * as updater from "nsh/updater";
import { Application } from "nsh/desktop";
import { collapseBreadcrumbs } from "./breadcrumbs";
import {
  type Scanner,
  type ScanProgress,
  type RemovalProgress,
  type DiskNode,
  type Snapshot,
  type DirectoryView,
  type Metric,
  mosaic,
  formatSize,
  kind,
  sizeStops,
  sizeColor,
} from "./disk";

import {
  Option,
  palette as C,
  finish,
  Button,
  MapTile,
  EmptyMap,
  MetricControl,
  AppMark,
  AccentBorder,
  iconTint,
  mono,
} from "./ui";
import { Icon } from "nsh-icon";
import {
  ArrowRight,
  Check,
  ChevronLeft,
  ChevronRight,
} from "nsh-icon-tabler";
// Runners built from released NSH 0.6.x (a TSX app with a Rust nshm) still
// open the main window themselves (window model v1) and predate the built-in
// update prompt, which landed together with model v2 on main.
const legacyRuntime = typeof updater.promptForUpdate !== "function";

export function App(props: { scanner: Scanner }) {
  if (legacyRuntime) {
    if (desktopAvailable)
      currentWindow().set({
        title: "Disktree — Disk space explorer",
        titleBar: { style: "hidden", height: 56 },
      });
    return <Explorer scanner={props.scanner} />;
  }
  return (
    <Window
      id="main"
      title="Disktree — Disk space explorer"
      titleBar={{ style: "hidden", height: 56 }}
      defaultSize={{ width: 1280, height: 850 }}
    >
      {() => <Explorer scanner={props.scanner} />}
    </Window>
  );
}

function Explorer(props: { scanner: Scanner }) {
  const { scanner } = props;
  let activeScan: string | undefined;
  cleanup(() => {
    if (activeScan) void scanner.cancelScan(activeScan).catch(() => {});
  });
  const snapshot = signal<Snapshot | null>(null);
  const current = signal<DiskNode | null>(null);
  const selection = signal<DiskNode | null>(null);
  const path = signal("");
  let pathField: ViewRef | undefined;
  let filterField: ViewRef | undefined;

  let breadcrumbOverflow: ViewRef | undefined;
  const crumbViewport = signal(0);
  const draftPath = signal("");
  const query = signal("");
  const busy = signal(false);
  const scanProgress = signal<ScanProgress | null>(null);
  const removalProgress = signal<RemovalProgress | null>(null);
  const removalStatus = () => {
    const p = removalProgress();
    if (!p) return "";
    if (removal() === "empty") return "Emptying Trash…";
    return (
      (removal() === "trash" ? "Moving to Trash" : "Deleting") +
      " · " +
      p.percent +
      "% · " +
      (p.files || 0) +
      " of " +
      (p.totalFiles || 0) +
      " files · " +
      formatSize(p.bytes || 0) +
      " of " +
      formatSize(p.totalBytes || 0)
    );
  };
  const scanSeconds = signal(0);
  let stopScanProgress: (() => void) | undefined;
  cleanup(() => stopScanProgress?.());
  async function scanWithProgress(target: string): Promise<Snapshot> {
    const started = Date.now();
    const progressToken = `${started}-${++scanSequence}`;
    activeScan = progressToken;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stop = () => {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      scanProgress.set(null);
    };
    stopScanProgress?.();
    stopScanProgress = stop;
    batch(() => {
      scanSeconds.set(0);
      scanProgress.set({
        path: target || "Opening saved folder…",
        percent: 0,
        directories: 0,
        files: 0,
        bytes: 0,
        errors: 0,
      });
    });
    async function poll() {
      try {
        const progress = await scanner.scanProgress(progressToken);
        if (stopped) return;
        batch(() => {
          if (progress) scanProgress.set(progress);
          scanSeconds.set(Math.floor((Date.now() - started) / 1000));
        });
      } catch {
        /* A progress update must not interrupt the scan. */
      }
      if (!stopped) timer = setTimeout(() => void poll(), 200);
    }
    timer = setTimeout(() => void poll(), 100);
    try {
      return await scanner.scan(target, progressToken);
    } finally {
      activeScan = undefined;
      stop();
      if (stopScanProgress === stop) stopScanProgress = undefined;
    }
  }
    async function promptUpdate() {
    if (!legacyRuntime) return updater.promptForUpdate();
    const update = await updater.checkForUpdate();
    if (!update) {
      message.set("Disktree is up to date.");
      return;
    }
    message.set(`Downloading Disktree ${update.version}…`);
    await updater.downloadUpdate();
    await updater.installAndRestart();
  }
  let scanSequence = 0;
  const message = signal("Choose a folder to discover what fills your disk.");
  const updateAvailable = signal(false);
  function showUpdates() {
    void promptUpdate().catch((error) => message.set(String(error)));
  }
  const hidden = signal(true);
  const metric = signal<Metric>("size");
  const listingMetric = signal<Metric>("size");
  const levels = signal(2);
  const optionsOpen = signal(false);
  const marked = signal<DiskNode[]>([]);
  const review = signal(false);
  const removal = signal<"trash" | "delete" | "empty">("trash");
  const editingPath = signal(false);
  const reviewed = signal<DiskNode[]>([]);
  function beginReview(mode: "trash" | "delete" | "empty", nodes = marked()) {
    if (busy() || browseBusy() || (mode !== "empty" && !nodes.length)) return;
    batch(() => {
      removal.set(mode);
      reviewed.set(nodes.slice());
      review.set(true);
    });
  }
  const pointerInMap = signal(false);
  const hovered = signal(new Map<string, DiskNode>());
  function hoverNode(node: DiskNode, inside: boolean) {
    hovered.update((previous) => {
      if (previous.has(node.path) === inside) return previous;
      const next = new Map(previous);
      if (inside) next.set(node.path, node);
      else next.delete(node.path);
      return next;
    });
  }
  function markAtPointer() {
    const nodes = Array.from(hovered().values()).sort(
      (a, b) => b.path.length - a.path.length,
    );
    toggleMark(nodes[0] || (pointerInMap() ? null : selection()));
  }
  const frame = signal(
    { width: 700, height: 520 },
    { equals: (a, b) => a.width === b.width && a.height === b.height },
  );
  const zoom = signal(1);
  const cameraScale = createAnimatedValue(1);
  const cameraX = createAnimatedValue(0);
  const cameraY = createAnimatedValue(0);
  const boundedScale = cameraScale.interpolate({
    inputRange: [1, 8],
    outputRange: [1, 8],
  });
  const cameraTransform = computed(() => {
    const maxX = boundedScale.sub(1).mul(frame().width / 2);
    const maxY = boundedScale.sub(1).mul(frame().height / 2);
    return [
      {
        translateX: cameraX
          .div(maxX.add(0.001))
          .interpolate({ inputRange: [-1, 1], outputRange: [-1, 1] })
          .mul(maxX),
      },
      {
        translateY: cameraY
          .div(maxY.add(0.001))
          .interpolate({ inputRange: [-1, 1], outputRange: [-1, 1] })
          .mul(maxY),
      },
      { scale: boundedScale },
    ];
  });
  const detailLevels = computed(() =>
    Math.max(levels(), zoom() >= 1.8 ? 3 : 1),
  );
  const listing = signal<DirectoryView | null>(null);
  const directoryPath = signal("");
  const page = signal(0);
  const browseBusy = signal(false);
  let viewGeneration = 0;
  let loadedViewKey = "";
  function viewParameters(
    id: string | undefined,
    folder: string,
    offset = page(),
  ) {
    return {
      id,
      path: folder,
      offset,
      query: query(),
      hidden: hidden(),
      metric: metric(),
      levels: detailLevels(),
      fallback: true,
    };
  }
  const entrance = createAnimatedValue(0);
  const sweep = createAnimatedValue(0);
  const panelReveal = createAnimatedValue(1);
  effect(() => {
    if (reduceMotion()) entrance.set(1);
    else void entrance.timing(1, { duration: 680, easing: [0.16, 1, 0.3, 1] });
  });
  effect(() => {
    review();
    selection()?.path;
    if (reduceMotion()) {
      panelReveal.set(1);
      return;
    }
    panelReveal.set(0);
    void panelReveal.timing(1, { duration: 220, easing: [0.16, 1, 0.3, 1] });
  });
  effect(() => {
    if (!busy() || reduceMotion()) {
      sweep.set(0);
      return;
    }
    const loop = Animated.loop(
      Animated.timing(sweep, { toValue: 1, duration: 1700, easing: "linear" }),
    );
    void loop.start();
    cleanup(() => loop.stop());
  });
  /** Checks, downloads and installs an update, recording each outcome for the smoke test. */
  async function scriptedUpdate() {
    const version = updater.shellInfo().version ?? "";
    const record = (outcome: string, text: string) =>
      scanner.recordUpdate?.(outcome, text, version).catch(() => {});
    try {
      const update = await updater.checkForUpdate();
      if (!update) return void (await record("current", "Disktree is up to date."));
      await record("available", update.version);
      if (!update.canInstall)
        return void (await record("failed", `Download the update from ${update.url}`));
      await updater.downloadUpdate();
      await updater.installAndRestart();
    } catch (error) {
      await record("failed", error instanceof Error ? error.message : String(error));
    }
  }
  /** Commands for unattended smoke tests: `wait <ms>`, `key mod+r`, `update`, `quit`. */
  async function runScript() {
    const script = (await scanner.uiScript?.().catch(() => "")) ?? "";
    const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
    for (const command of script.split(";").map((c) => c.trim()).filter(Boolean)) {
      const [name, arg] = command.split(/\s+/);
      if (name === "wait") await sleep(Number(arg) || 0);
      else if (name === "key" && (arg === "mod+r" || arg === "cmd+r")) {
        while (busy()) await sleep(50);
        void scan();
      } else if (name === "update" || name === "update-auto") {
        await scriptedUpdate();
      } else if (name === "quit") {
        Application.quit();
        return;
      } else console.warn(`Unknown DISKTREE_UI_SCRIPT command: ${command}`);
    }
  }
  let generation = 0;
  async function scan(target = path()) {
    if (busy()) return;
    const run = ++generation;
    ++viewGeneration;
    browseBusy.set(false);
    busy.set(true);
    message.set("Scanning… large folders may take a moment.");
    try {
      const data: Snapshot = await scanWithProgress(target);
      if (run !== generation) return;
      batch(() => {
        loadedViewKey = "";
        listing.set(null);
        snapshot.set(data);
        current.set(data.root);
        selection.set(data.root);
        marked.set([]);
        review.set(false);
        path.set(data.root.path);
        directoryPath.set(data.root.path);
        page.set(0);
        query.set("");
      });
      message.set(
        data.errorCount || data.errors.length
          ? `${data.errorCount || data.errors.length} unreadable or skipped paths. Scan totals exclude these items.`
          : "Scan complete · symbolic links and other volumes are not followed.",
      );
    } catch (error) {
      message.set(
        `${error instanceof Error ? error.message : error}. Restart the app and try again.`,
      );
    } finally {
      busy.set(false);
    }
  }
  function enter(node: DiskNode) {
    if (busy() || browseBusy()) return;
    if (node.aggregate) {
      message.set(
        "Use the directory list and page controls to explore the remaining items.",
      );
      return;
    }
    selection.set(node);
    if (node.directory)
      batch(() => {
        directoryPath.set(node.path);
        page.set(0);
        query.set("");
      });
  }
  function contextMenu(node: DiskNode, point: { x: number; y: number }) {
    const isMarked = marked().some((n) => n.path === node.path);
    void Menu.popup(
      [
        {
          label: "Show details",
          disabled: !!node.aggregate,
          onSelect: () => selection.set(node),
        },
        {
          label: "Open folder",
          disabled: !node.directory,
          onSelect: () => enter(node),
        },
        {
          label: isMarked ? "Unmark" : "Mark for removal",
          disabled: !!node.aggregate || busy(),
          onSelect: () => toggleMark(node),
        },
        {
          label: "Review " + marked().length + " marked…",
          disabled: !marked().length,
          onSelect: () => beginReview("trash"),
        },
      ],
      point,
    );
  }
  function selectOrEnter(node: DiskNode) {
    if (busy() || browseBusy()) return;
    if (node.aggregate || (selection()?.path === node.path && node.directory))
      enter(node);
    else selection.set(node);
  }
  effect(() => {
    const data = snapshot();
    const folder = directoryPath();
    const parameters = viewParameters(data?.id, folder);
    const key = JSON.stringify(parameters);
    if (busy() || !data || !folder || key === loadedViewKey) return;
    const run = ++viewGeneration;
    browseBusy.set(true);
    void scanner
      .view(parameters)
      .then((result: DirectoryView) => {
        if (run !== viewGeneration) return;
        loadedViewKey = JSON.stringify({
          ...parameters,
          path: result.node.path,
          offset: result.offset,
        });
        batch(() => {
          listing.set(result);
          listingMetric.set(parameters.metric);
          const previous = current()?.path;
          current.set(result.node);
          if (previous !== result.node.path || selection()?.path === previous)
            selection.set(result.node);
          path.set(result.node.path);
          directoryPath.set(result.node.path);
          page.set(result.offset);
          browseBusy.set(false);
        });
      })
      .catch((error) => {
        if (run === viewGeneration) {
          browseBusy.set(false);
          message.set(String(error));
        }
      });
  });
  function up() {
    if (busy() || browseBusy()) return;
    const folder = current()?.path;
    if (!folder || folder === "/") return;
    const ancestors = listing()?.ancestors || [];
    if (ancestors.length > 1) enter(ancestors[ancestors.length - 2]);
    else void scan(folder.slice(0, folder.lastIndexOf("/")) || "/");
  }
  function toggleMark(node: DiskNode | null) {
    if (
      !node ||
      node.aggregate ||
      node.path === snapshot()?.root.path ||
      busy() ||
      browseBusy()
    )
      return;
    if (marked().some((n) => n.path === node.path)) {
      marked.set(marked().filter((n) => n.path !== node.path));
      return;
    }
    if (marked().some((n) => node.path.startsWith(n.path + "/"))) {
      message.set("A parent folder is already marked.");
      return;
    }
    marked.set([
      ...marked().filter((n) => !n.path.startsWith(node.path + "/")),
      node,
    ]);
  }

  const children = computed(() => listing()?.children || []);
  const tiles = computed(() =>
    mosaic(
      listing()?.map || [],
      frame().width,
      frame().height,
      listingMetric(),
      detailLevels(),
      zoom(),
    ),
  );
  let previousTiles: ReturnType<typeof mosaic> = [];
  effect(() => {
    const next = tiles();
    const changed =
      next.length !== previousTiles.length ||
      next.some((tile, i) => {
        const old = previousTiles[i];
        return (
          !old ||
          tile.node.path !== old.node.path ||
          tile.x !== old.x ||
          tile.y !== old.y ||
          tile.width !== old.width ||
          tile.height !== old.height
        );
      });
    previousTiles = next;
    if (changed && !reduceMotion()) {
      // Animate the commit that changes the keyed tiles. Native layout handles
      // positions/sizes; no JS frame loop or idle animation is needed.
      LayoutAnimation.configureNext({
        duration: 320,
        create: { type: "easeOut", property: "opacity" },
        update: { type: "easeInEaseOut" },
        delete: { type: "easeOut", property: "opacity", duration: 160 },
      });
    }
  });
  const crumbs = computed(() => {
    const parts = path().split("/").filter(Boolean);
    return [
      { name: "/", path: "/" },
      ...parts.map((name: string, i: number) => ({
        name,
        path: "/" + parts.slice(0, i + 1).join("/"),
      })),
    ];
  });
  const collapsedCrumbs = computed(() =>
    collapseBreadcrumbs(crumbs(), crumbViewport()),
  );
  async function showBreadcrumbs() {
    const anchor = await breadcrumbOverflow?.measure();
    await Menu.popup(
      collapsedCrumbs().hidden.map((crumb) => ({
        label: crumb.path,
        enabled: !busy() && !browseBusy(),
        onSelect: () => navigate(crumb.path),
      })),
      anchor
        ? {
            x: anchor.pagePosition.x,
            y: anchor.pagePosition.y + anchor.bounds.size.height + 4,
          }
        : undefined,
    );
  }
  function navigate(target: string) {
    if (busy() || browseBusy()) return;
    const node = listing()?.ancestors.find((n) => n.path === target);
    if (node) enter(node);
    else void scan(target);
  }
  function changePage(offset: number) {
    page.set(Math.max(0, offset));
  }
  function zoomMap(scale: number) {
    const next = Math.max(1, Math.min(8, scale));
    zoom.set(next);
    if (reduceMotion()) cameraScale.set(next);
    else
      void cameraScale.timing(next, {
        duration: 220,
        easing: [0.16, 1, 0.3, 1],
      });
    if (next === 1) {
      cameraX.set(0);
      cameraY.set(0);
    }
  }
  function endPinch(scale: number) {
    const next = Math.max(1, Math.min(8, scale));
    cameraScale.set(next);
    zoom.set(next);
    if (next === 1) {
      cameraX.set(0);
      cameraY.set(0);
    }
  }
  effect(() => {
    directoryPath();
    page();
    metric();
    hovered.set(new Map());
    zoom.set(1);
    cameraScale.set(1);
    cameraX.set(0);
    cameraY.set(0);
  });
  async function commit() {
    if (
      busy() ||
      browseBusy() ||
      (removal() !== "empty" && (!snapshot() || !reviewed().length))
    )
      return;
    ++viewGeneration;
    busy.set(true);
    let outcome = "";
    removalProgress.set({
      percent: 0,
      items: 0,
      totalItems: reviewed().length,
    });
    const progressToken = "remove-" + Date.now();
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function pollRemoval() {
      try {
        const progress = await scanner.removalProgress(progressToken);
        if (active && progress) removalProgress.set(progress);
      } catch {
        /* The operation reports its own errors. */
      }
      if (active) timer = setTimeout(() => void pollRemoval(), 200);
    }
    timer = setTimeout(() => void pollRemoval(), 200);
    try {
      const mode = removal();
      const result = await scanner.remove(
        mode,
        snapshot()?.id,
        reviewed().map((n) => n.path),
        progressToken,
      );
      outcome =
        result.error ||
        (mode === "empty"
          ? "Trash emptied."
          : `${Array.isArray(result.completed) ? result.completed.length : 0} items ${mode === "delete" ? "permanently deleted" : "moved to Trash"}.`);
      const completed: string[] = Array.isArray(result.completed)
        ? result.completed
        : [];
      const removed = (node: DiskNode) =>
        completed.some((p) => node.path === p || node.path.startsWith(p + "/"));
      batch(() => {
        const selected = selection();
        if (selected && removed(selected)) selection.set(current());
        if (result.snapshot && snapshot())
          snapshot.set({ ...snapshot()!, ...result.snapshot });
        loadedViewKey = "";
        review.set(!!result.error);
        reviewed.set(reviewed().filter((n) => !removed(n)));
        marked.set(marked().filter((n) => !removed(n)));
        hovered.set(new Map());
      });
      if (result.warning) outcome += ` ${result.warning}`;
      message.set(outcome);
    } catch (error) {
      message.set(
        outcome
          ? `${outcome} Refresh failed: ${String(error)}. Scan again to update disk usage.`
          : String(error),
      );
    } finally {
      active = false;
      clearTimeout(timer);
      removalProgress.set(null);
      busy.set(false);
    }
  }
  keymap.bind(
    {
      "mod+plus": () => zoomMap(zoom() * 1.5),
      "mod+minus": () => zoomMap(zoom() / 1.5),
      "mod+0": () => zoomMap(1),
      "alt+up": up,
      enter: () => {
        if (selection()) enter(selection()!);
      },
      space: markAtPointer,
      "mod+r": () => {
        void scan();
      },
      down: () => {
        const list = children();
        const i = list.findIndex((n) => n.path === selection()?.path);
        if (list.length) selection.set(list[Math.min(i + 1, list.length - 1)]);
      },
      up: () => {
        const list = children();
        const i = list.findIndex((n) => n.path === selection()?.path);
        if (list.length) selection.set(list[Math.max(i - 1, 0)]);
      },
    },
    { when: "notEditing" },
  );
  keymap.bind(
    "escape",
    () => {
      if (!busy()) {
        if (optionsOpen()) optionsOpen.set(false);
        else if (review()) review.set(false);
        else selection.set(current());
        editingPath.set(false);
        filterField?.blur();
      }
    },
    { when: "always" },
  );
  // Start the request during initialization so the first frame is already scanning.
  void scan();
  void runScript();
  void updater
    .checkForUpdate()
    .then((update) => {
      updateAvailable.set(!!update);
    })
    .catch(() => {});
  return (
    <View style={{ flex: 1, backgroundColor: C.bg, opacity: entrance }}>
      <View
        desktop={{ windowDrag: true }}
        testID="titlebar"
        style={{
          height: 56,
          flexDirection: "row",
          alignItems: "center",
          gap: 10,
          paddingLeft: Math.max(
            16,
            (desktopAvailable ? titleBar().left : 0) + 8,
          ),
          paddingRight: Math.max(
            16,
            (desktopAvailable ? titleBar().right : 0) + 8,
          ),
          borderBottomWidth: 1,
          borderColor: C.line,
          backgroundColor: "#111113",
          ...finish,
        }}
      >
        <AppMark />
        <Text
          style={{
            fontFamily: mono,
            fontSize: 17,
            fontWeight: "600",
            letterSpacing: -0.5,
            color: C.text,
          }}
        >
          disktree
        </Text>
        <View style={{ flex: 1 }} />
        <Button
          label="Edit folder path"
          icon="edit"
          disabled={busy()}
          action={() => {
            draftPath.set(path());
            editingPath.update((v) => !v);
          }}
        />
        <Button
          label="Scan folder"
          icon="scan"
          disabled={busy()}
          action={() => {
            const target = editingPath() ? draftPath() : path();
            editingPath.set(false);
            void scan(target);
          }}
          active
        />
        <Button
          label="Parent folder"
          icon="parent"
          disabled={busy() || browseBusy() || current()?.path === "/"}
          action={up}
        />
        <MetricControl
          value={metric()}
          change={(value) =>
            batch(() => {
              metric.set(value);
              page.set(0);
            })
          }
        />
        <OptionsMenu
          open={optionsOpen()}
          onOpenChange={(e) => optionsOpen.set(e.open)}
          placement="bottom"
          align="end"
          offset={6}
          popupStyle={{
            minWidth: 190,
            padding: 4,
            backgroundColor: C.panel,
            borderWidth: 1,
            borderColor: C.line,
            borderRadius: 6,
          }}
          trigger={<Button label="View options" icon="options" />}
        >
          <Option
            label="Show hidden files"
            checked={hidden()}
            action={() =>
              batch(() => {
                hidden.update((v) => !v);
                page.set(0);
              })
            }
          />
          <Each of={[1, 2, 3]}>
            {(n) => (
              <Option
                label={n() + " level" + (n() === 1 ? "" : "s")}
                checked={levels() === n()}
                action={() => levels.set(n())}
              />
            )}
          </Each>
          <Option label="Check for updates…" action={showUpdates} />
          <Option
            label="Empty Trash…"
            disabled={busy() || browseBusy()}
            action={() => beginReview("empty")}
          />
        </OptionsMenu>
      </View>
      <View
        style={{
          height: 2,
          overflow: "hidden",
          backgroundColor: busy() ? C.line : "transparent",
        }}
      >
        <If when={busy()}>
          <View
            style={{
              height: 2,
              width: reduceMotion() ? "100%" : "28%",
              backgroundColor: C.amber,
              transform: [
                {
                  translateX: sweep.interpolate({
                    inputRange: [0, 1],
                    outputRange: [-300, 1500],
                  }),
                },
              ],
            }}
          />
        </If>
      </View>
      <View style={{ flex: 1, flexDirection: "row", minHeight: 150 }}>
        <View style={{ flex: 1, padding: 10, gap: 7 }}>
          <View
            style={{
              flexDirection: "row",
              alignItems: "center",
              gap: 8,
              height: 28,
            }}
          >
            <View
              desktop={{ windowDrag: false }}
              style={{ flex: 1, minWidth: 80, overflow: "hidden" }}
              onLayout={(e) => crumbViewport.set(e.bounds.size.width)}
            >
              <If when={!editingPath()}>
                <View
                  testID="breadcrumbs"
                  style={{
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 3,
                    overflow: "hidden",
                  }}
                >
                  <View
                    desktop={{ windowDrag: false }}
                    role="button"
                    accessibilityLabel="Go to /"
                    accessibilityState={{
                      disabled: busy() || browseBusy() || path() === "/",
                    }}
                    onPress={() => navigate("/")}
                    class="hover:brightness-125"
                    style={{ padding: 6, flexShrink: 0, cursor: "pointer" }}
                  >
                    <Text
                      style={{
                        fontFamily: mono,
                        fontSize: 12,
                        color: path() === "/" ? C.text : C.amber,
                      }}
                    >
                      /
                    </Text>
                  </View>
                  <If when={collapsedCrumbs().hidden.length > 0}>
                    <Icon
                      icon={ChevronRight}
                      size={12}
                      style={{ pointerEvents: "none", flexShrink: 0 }}
                      class={iconTint(C.muted)}
                    />
                    <View
                      ref={(node) => {
                        breadcrumbOverflow = node;
                      }}
                      desktop={{ windowDrag: false }}
                      role="button"
                      accessibilityLabel="Show hidden path segments"
                      accessibilityState={{ disabled: busy() || browseBusy() }}
                      onPress={() => void showBreadcrumbs()}
                      class="hover:brightness-125"
                      style={{
                        paddingHorizontal: 6,
                        paddingVertical: 4,
                        borderRadius: 3,
                        flexShrink: 0,
                        cursor: "pointer",
                      }}
                    >
                      <Text
                        style={{
                          fontFamily: mono,
                          fontSize: 14,
                          color: C.amber,
                        }}
                      >
                        …
                      </Text>
                    </View>
                  </If>
                  <Each of={collapsedCrumbs().tail}>
                    {(n) => (
                      <View
                        style={{
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 3,
                          flexShrink: 1,
                          minWidth: 0,
                        }}
                      >
                        <Icon
                          icon={ChevronRight}
                          size={12}
                          style={{ pointerEvents: "none", flexShrink: 0 }}
                          class={iconTint(C.muted)}
                        />
                        <View
                          desktop={{ windowDrag: false }}
                          role="button"
                          accessibilityLabel={`Go to ${n().path}`}
                          accessibilityState={{
                            disabled:
                              busy() ||
                              browseBusy() ||
                              n().path === current()?.path,
                          }}
                          onPress={() => {
                            if (n().path !== current()?.path)
                              navigate(n().path);
                          }}
                          class="hover:brightness-125"
                          style={{
                            padding: 6,
                            borderRadius: 3,
                            flexShrink: 1,
                            minWidth: 0,
                            cursor:
                              n().path === current()?.path
                                ? "default"
                                : "pointer",
                          }}
                        >
                          <Text
                            style={{
                              pointerEvents: "none",
                              numberOfLines: 1,
                              textOverflow: "ellipsis-middle",
                              fontFamily: mono,
                              fontSize: 12,
                              maxWidth: 180,
                              flexShrink: 1,
                              color:
                                n().path === current()?.path ? C.text : C.amber,
                            }}
                          >
                            {n().name}
                          </Text>
                        </View>
                      </View>
                    )}
                  </Each>
                </View>
              </If>
              <If when={editingPath()}>
                <Input
                  ref={(node) => {
                    pathField = node;
                    node.focus();
                  }}
                  accessibilityLabel="Folder path"
                  value={draftPath()}
                  onChange={({ value: v }) => draftPath.set(v)}
                  onSubmit={() => {
                    editingPath.set(false);
                    void scan(draftPath());
                  }}
                  autoCorrect={false}
                  autoCapitalize="none"
                  style={{
                    fontFamily: mono,
                    color: C.text,
                    fontSize: 12,
                    padding: 8,
                    backgroundColor: "#09090b",
                  }}
                />
              </If>
            </View>
            <Text style={{ fontFamily: mono, fontSize: 11, color: C.muted }}>
              {current()
                ? `${formatSize(current()!.size)} · ${current()!.files.toLocaleString()} files`
                : ""}
              {browseBusy() ? " · Loading…" : ""}
            </Text>
            <Input
              accessibilityLabel="Filter names"
              ref={(node) => {
                filterField = node;
              }}
              value={query()}
              onChange={({ value: v }) =>
                batch(() => {
                  query.set(v);
                  page.set(0);
                })
              }
              placeholder="Filter names…"
              style={{
                placeholderColor: C.muted,
                fontFamily: mono,
                width: 130,
                fontSize: 11,
                color: C.text,
                padding: 6,
                backgroundColor: "#18181c",
                borderRadius: 4,
              }}
            />
          </View>
          <Scroll horizontal style={{ height: 20, flexGrow: 0, flexShrink: 0 }}>
            <View
              style={{ flexDirection: "row", gap: 13, alignItems: "center" }}
            >
              <Each of={sizeStops}>
                {(category) => (
                  <View
                    style={{
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 4,
                    }}
                  >
                    <View
                      style={{
                        width: 8,
                        height: 8,
                        borderRadius: 1,
                        backgroundColor: category().color,
                      }}
                    />
                    <Text
                      style={{ fontFamily: mono, fontSize: 10, color: C.muted }}
                    >
                      {category().label}
                    </Text>
                  </View>
                )}
              </Each>
            </View>
          </Scroll>
          <View
            testID="treemap"
            class="hover:opacity-100"
            onMouseEnter={() => {
              pointerInMap.set(true);
              pathField?.blur();
              filterField?.blur();
            }}
            onMouseLeave={() => {
              pointerInMap.set(false);
              hovered.set(new Map());
            }}
            pinch={{ scale: cameraScale }}
            pan={{ x: cameraX, y: cameraY }}
            onPinchEnd={() => {
              void cameraScale.getValue().then(endPinch);
            }}
            onLayout={(e) =>
              frame.set({
                width: e.bounds.size.width,
                height: e.bounds.size.height,
              })
            }
            style={{
              flex: 1,
              overflow: "hidden",
              backgroundColor: "#050506",
              borderRadius: 6,
            }}
          >
            <View
              style={{
                width: frame().width,
                height: frame().height,
                transform: cameraTransform(),
              }}
            >
              <Each of={tiles()} key={(tile) => tile.node.path}>
                {(tile) => (
                  <MapTile
                    tile={tile()}
                    selected={selection()?.path === tile().node.path}
                    marked={marked().some(
                      (n) =>
                        n.path === tile().node.path ||
                        tile().node.path.startsWith(n.path + "/"),
                    )}
                    dimmed={
                      !!selection() &&
                      selection()?.path !== current()?.path &&
                      selection()?.path !== tile().node.path
                    }
                    metric={listingMetric()}
                    levels={detailLevels()}
                    zoom={zoom()}
                    hover={() => hoverNode(tile().node, true)}
                    leave={() => hoverNode(tile().node, false)}
                    choose={() => selectOrEnter(tile().node)}
                    open={() => enter(tile().node)}
                    context={(e) => contextMenu(tile().node, e.pagePosition)}
                  />
                )}
              </Each>
            </View>
            <If when={!tiles().length}>
              <View
                style={{
                  pointerEvents: "none",
                  position: "absolute",
                  left: 0,
                  right: 0,
                  top: 0,
                  bottom: 0,
                }}
              >
                <EmptyMap progress={scanProgress()} seconds={scanSeconds()} />
              </View>
            </If>
            <AccentBorder radius={3} />
          </View>
        </View>
        <View
          style={{
            width: 284,
            flexShrink: 0,
            backgroundColor: C.panel,
            borderLeftWidth: 1,
            borderColor: C.line,
          }}
        >
          <Scroll style={{ flex: 1 }}>
            <View
              style={{
                padding: 16,
                gap: 12,
                opacity: panelReveal,
                transform: [
                  {
                    translateX: panelReveal.interpolate({
                      inputRange: [0, 1],
                      outputRange: [8, 0],
                    }),
                  },
                ],
              }}
            >
              <Text
                style={{
                  fontFamily: mono,
                  fontSize: 10,
                  letterSpacing: 1,
                  color: C.muted,
                }}
              >
                {review()
                  ? removal() === "empty"
                    ? "EMPTY TRASH"
                    : "REVIEW REMOVAL"
                  : selection()?.path === current()?.path
                    ? "FOLDER CONTENTS"
                    : "SELECTION"}
              </Text>
              <If when={!review() && selection()}>
                <If when={selection()?.path !== current()?.path}>
                  <Button
                    label="Back to contents"
                    action={() => selection.set(current())}
                  />
                </If>
                <View
                  style={{ flexDirection: "row", alignItems: "center", gap: 8 }}
                >
                  <View
                    style={{
                      width: 3,
                      height: 19,
                      backgroundColor: selection()
                        ? sizeColor(selection()!.size)
                        : C.amber,
                    }}
                  />
                  <Text
                    style={{
                      fontFamily: mono,
                      fontSize: 20,
                      fontWeight: "600",
                      color: C.text,
                      flex: 1,
                      numberOfLines: 1,
                    }}
                  >
                    {selection()?.name || "Nothing selected"}
                  </Text>
                </View>
                <Text
                  style={{ fontFamily: mono, fontSize: 11, color: C.muted }}
                >
                  {selection()?.path || "Select a tile to inspect it."}
                </Text>
                <Text
                  style={{
                    fontFamily: mono,
                    fontSize: 38,
                    fontWeight: "500",
                    letterSpacing: -1,
                    color: "#e8e8ed",
                    fontVariant: ["tabular-nums"],
                  }}
                >
                  {listingMetric() === "files"
                    ? selection()?.files
                    : formatSize(selection()?.[listingMetric()] || 0)}
                </Text>
                <View
                  style={{
                    height: 4,
                    backgroundColor: "#26262b",
                    borderRadius: 2,
                  }}
                >
                  <View
                    style={{
                      height: 4,
                      width: `${(100 * (selection()?.size || 0)) / (snapshot()?.root.size || 1)}%`,
                      backgroundColor: selection()
                        ? sizeColor(selection()!.size)
                        : C.amber,
                      borderRadius: 2,
                    }}
                  />
                </View>
                <View style={{ flexDirection: "row", gap: 16 }}>
                  <View style={{ flex: 1, gap: 5 }}>
                    <Text
                      style={{ fontFamily: mono, fontSize: 10, color: C.muted }}
                    >
                      OF SCAN
                    </Text>
                    <Text
                      style={{ fontFamily: mono, fontSize: 15, color: C.text }}
                    >
                      {(
                        ((selection()?.size || 0) /
                          (snapshot()?.root.size || 1)) *
                        100
                      ).toFixed(1)}
                      %
                    </Text>
                  </View>
                  <View style={{ flex: 1, gap: 5 }}>
                    <Text
                      style={{ fontFamily: mono, fontSize: 10, color: C.muted }}
                    >
                      FILES
                    </Text>
                    <Text
                      style={{ fontFamily: mono, fontSize: 15, color: C.text }}
                    >
                      {(selection()?.files || 0).toLocaleString()}
                    </Text>
                  </View>
                </View>
                <View style={{ flexDirection: "row", gap: 16 }}>
                  <View style={{ flex: 1, gap: 5 }}>
                    <Text
                      style={{ fontFamily: mono, fontSize: 10, color: C.muted }}
                    >
                      MODIFIED
                    </Text>
                    <Text
                      style={{ fontFamily: mono, fontSize: 12, color: C.text }}
                    >
                      {selection()
                        ? new Date(
                            selection()!.modified * 1000,
                          ).toLocaleDateString()
                        : "—"}
                    </Text>
                  </View>
                  <View style={{ flex: 1, gap: 5 }}>
                    <Text
                      style={{ fontFamily: mono, fontSize: 10, color: C.muted }}
                    >
                      {selection()?.path === current()?.path
                        ? "LARGEST"
                        : "KIND"}
                    </Text>
                    <Text
                      style={{ fontFamily: mono, fontSize: 12, color: C.text }}
                    >
                      {selection()?.path === current()?.path
                        ? children()[0]?.name || "—"
                        : kind(selection()!).label}
                    </Text>
                  </View>
                </View>
                <If when={selection()?.path !== current()?.path}>
                  <View style={{ flexDirection: "row", gap: 6 }}>
                    <Button
                      label="Open folder"
                      disabled={
                        !selection()?.directory || busy() || browseBusy()
                      }
                      action={() => {
                        if (selection()) enter(selection()!);
                      }}
                    />
                    <Button
                      label={
                        marked().some((n) => n.path === selection()?.path)
                          ? "Unmark"
                          : "Mark"
                      }
                      action={() => toggleMark(selection())}
                    />
                  </View>
                </If>
              </If>
              <If when={review()}>
                <Text style={{ fontFamily: mono, fontSize: 15, color: C.text }}>
                  {removal() === "empty"
                    ? "Empty the system Trash?"
                    : removal() === "delete"
                      ? "Permanently delete these items?"
                      : "Move these items to Trash?"}
                </Text>
                <Text
                  style={{ fontFamily: mono, fontSize: 11, color: C.muted }}
                >
                  {removal() === "trash"
                    ? "Recoverable from the system Trash."
                    : "This permanently removes files and cannot be undone."}
                </Text>
                <Each of={removal() === "empty" ? [] : reviewed()}>
                  {(node) => (
                    <View style={{ gap: 7, paddingVertical: 6 }}>
                      <Text
                        style={{
                          fontFamily: mono,
                          fontSize: 11,
                          color: C.text,
                        }}
                      >
                        {node().path}
                      </Text>
                      <Button
                        label={`Exclude · ${formatSize(node().size)}`}
                        accessibilityLabel={`Exclude ${node().path}`}
                        disabled={busy()}
                        action={() => {
                          if (!busy()) {
                            reviewed.set(
                              reviewed().filter((n) => n.path !== node().path),
                            );
                          }
                        }}
                      />
                    </View>
                  )}
                </Each>
                <If when={removalProgress()}>
                  <Text
                    testID="removal-progress"
                    style={{ color: C.text, fontFamily: mono }}
                  >
                    {removalProgress()!.percent}% · {removalProgress()!.items} /{" "}
                    {removalProgress()!.totalItems} items · {removalStatus()} ·{" "}
                    {removalProgress()!.current}
                  </Text>
                </If>
                <Button
                  label={
                    busy()
                      ? "Working…"
                      : removal() === "empty"
                        ? "Confirm empty Trash"
                        : removal() === "delete"
                          ? "Confirm permanent deletion"
                          : "Confirm move to Trash"
                  }
                  action={() => void commit()}
                  danger={removal() !== "trash"}
                  active={removal() === "trash"}
                  disabled={
                    busy() ||
                    browseBusy() ||
                    (removal() !== "empty" && !reviewed().length)
                  }
                />
                <If when={removal() === "trash"}>
                  <Button
                    label="Delete permanently"
                    danger
                    disabled={busy() || browseBusy() || !reviewed().length}
                    action={() => removal.set("delete")}
                  />
                </If>
                <Button
                  label="Back to explorer"
                  disabled={busy()}
                  action={() => review.set(false)}
                />
              </If>
              <View style={{ height: 1, backgroundColor: C.line }} />
              <If when={!review() && selection()?.path === current()?.path}>
                <View style={{ height: 1, backgroundColor: C.line }} />
                <View
                  style={{ flexDirection: "row", alignItems: "center", gap: 5 }}
                >
                  <Text
                    style={{
                      fontFamily: mono,
                      flex: 1,
                      fontSize: 10,
                      letterSpacing: 0.8,
                      color: C.muted,
                    }}
                  >
                    CONTENTS · {listing()?.count || 0} ITEMS
                  </Text>
                  <Text
                    style={{ fontFamily: mono, fontSize: 10, color: C.muted }}
                  >
                    {listing()?.count
                      ? `${listing()!.offset + 1}–${Math.min(listing()!.offset + listing()!.pageSize, listing()!.count)}`
                      : ""}
                  </Text>
                </View>
                <If
                  when={(listing()?.count || 0) > (listing()?.pageSize || 80)}
                >
                  <View
                    style={{
                      flexDirection: "row",
                      justifyContent: "space-between",
                    }}
                  >
                    <Button
                      label="Previous"
                      leading={ChevronLeft}
                      disabled={page() === 0 || browseBusy()}
                      action={() =>
                        changePage(page() - (listing()?.pageSize || 80))
                      }
                    />
                    <Button
                      label="Next"
                      trailing={ChevronRight}
                      disabled={
                        browseBusy() ||
                        (listing()?.offset || 0) +
                          (listing()?.pageSize || 80) >=
                          (listing()?.count || 0)
                      }
                      action={() => {
                        const v = listing();
                        if (v && v.offset + v.pageSize < v.count)
                          changePage(v.offset + v.pageSize);
                      }}
                    />
                  </View>
                </If>
                <If when={!children().length}>
                  <Text style={{ color: C.muted, fontFamily: mono }}>
                    {busy()
                      ? "Scanning…"
                      : query()
                        ? "No names match “" + query() + "”"
                        : "This folder is empty"}
                  </Text>
                </If>
                <Each of={children()} key={(node) => node.path}>
                  {(node) => (
                    <View
                      role="button"
                      accessibilityLabel={"Select " + node().name}
                      onContextMenu={(e) => contextMenu(node(), e.pagePosition)}
                      onMouseEnter={() => hoverNode(node(), true)}
                      onMouseLeave={() => hoverNode(node(), false)}
                      onPress={() => selectOrEnter(node())}
                      class="hover:brightness-125"
                      style={{
                        paddingVertical: 7,
                        gap: 5,
                        cursor: "pointer",
                        borderBottomWidth: 1,
                        borderColor: "#242428",
                      }}
                    >
                      <View
                        style={{
                          flexDirection: "row",
                          gap: 6,
                          alignItems: "center",
                        }}
                      >
                        <View
                          style={{
                            width: 3,
                            height: 13,
                            backgroundColor: sizeColor(node().size),
                          }}
                        />
                        <If
                          when={marked().some((n) => n.path === node().path)}
                        >
                          <Icon
                            icon={Check}
                            size={12}
                            class={iconTint(
                              selection()?.path === node().path
                                ? C.amber
                                : C.text,
                            )}
                          />
                        </If>
                        <If when={node().directory}>
                          <Icon
                            icon={ChevronRight}
                            size={12}
                            class={iconTint(C.muted)}
                          />
                        </If>
                        <Text
                          style={{
                            numberOfLines: 1,
                            fontFamily: mono,
                            flex: 1,
                            fontSize: 11,
                            color:
                              selection()?.path === node().path
                                ? C.amber
                                : C.text,
                          }}
                        >
                          {node().name}
                        </Text>
                        <Text
                          style={{
                            fontFamily: mono,
                            fontSize: 10,
                            color: C.muted,
                          }}
                        >
                          {listingMetric() === "files"
                            ? String(node().files)
                            : formatSize(node()[listingMetric()])}
                        </Text>
                      </View>
                      <View style={{ height: 2, backgroundColor: "#101012" }}>
                        <View
                          style={{
                            height: 2,
                            width: `${(100 * node()[listingMetric()]) / (current()?.[listingMetric()] || 1)}%`,
                            backgroundColor: sizeColor(node().size),
                          }}
                        />
                      </View>
                    </View>
                  )}
                </Each>
              </If>
            </View>
          </Scroll>
          <If when={!review() && marked().length > 0}>
            <View
              style={{
                flexDirection: "row",
                alignItems: "center",
                gap: 8,
                padding: 12,
                borderTopWidth: 1,
                borderColor: C.line,
                backgroundColor: "#17150f",
              }}
            >
              <Text
                style={{
                  flex: 1,
                  fontFamily: mono,
                  color: C.text,
                  fontSize: 12,
                }}
              >
                {marked().length} marked ·{" "}
                {formatSize(marked().reduce((sum, n) => sum + n.size, 0))}
              </Text>
              <Button
                label="Review marked items"
                trailing={ArrowRight}
                action={() => beginReview("trash")}
                disabled={busy() || browseBusy()}
                active
              />
            </View>
          </If>

          <View
            style={{
              padding: 16,
              gap: 9,
              borderTopWidth: 1,
              borderColor: C.line,
              backgroundColor: "#141416",
              ...finish,
            }}
          >
            <Text
              style={{
                fontFamily: mono,
                fontSize: 10,
                letterSpacing: 1,
                color: C.muted,
              }}
            >
              DISK SPACE
            </Text>
            <Text
              style={{
                fontFamily: mono,
                fontSize: 25,
                fontWeight: "500",
                color: C.text,
                fontVariant: ["tabular-nums"],
              }}
            >
              {snapshot() ? formatSize(snapshot()!.free) : "—"}
              <Text style={{ fontFamily: mono, fontSize: 12, color: C.muted }}>
                {" "}
                free
              </Text>
            </Text>
            <View
              style={{ height: 5, backgroundColor: "#28282d", borderRadius: 2 }}
            >
              <View
                style={{
                  height: 5,
                  width: `${snapshot() ? 100 * (1 - snapshot()!.free / snapshot()!.total) : 0}%`,
                  backgroundColor: "#56b675",
                  borderRadius: 2,
                }}
              />
            </View>
            <View
              style={{ flexDirection: "row", justifyContent: "space-between" }}
            >
              <Text style={{ fontFamily: mono, fontSize: 10, color: C.muted }}>
                {snapshot()
                  ? `${formatSize(snapshot()!.total - snapshot()!.free)} used`
                  : ""}
              </Text>
              <Text style={{ fontFamily: mono, fontSize: 10, color: C.muted }}>
                {snapshot() ? `${formatSize(snapshot()!.total)} total` : ""}
              </Text>
            </View>
          </View>
        </View>
      </View>
      <View
        testID="statusbar"
        style={{
          flexDirection: "row",
          gap: 16,
          alignItems: "center",
          paddingHorizontal: 12,
          height: 28,
          flexShrink: 0,
          borderTopWidth: 1,
          borderColor: C.line,
          backgroundColor: "#101012",
        }}
      >
        <If when={updateAvailable()}>
          <Button label="Update available" action={showUpdates} />
        </If>
        <If when={scanProgress()}>
          <View
            testID="scan-progress"
            style={{
              flex: 1,
              flexDirection: "row",
              alignItems: "center",
              gap: 12,
            }}
          >
            <Text style={{ fontFamily: mono, fontSize: 10, color: C.amber }}>
              Scanning folders · ~{scanProgress()!.percent}%
            </Text>
            <Text
              style={{
                fontFamily: mono,
                fontSize: 10,
                color: C.muted,
                numberOfLines: 1,
                flex: 1,
              }}
            >
              {scanProgress()!.directories} folders · {scanProgress()!.files}{" "}
              files · {formatSize(scanProgress()!.bytes)} found ·{" "}
              {scanSeconds()}s elapsed
            </Text>
          </View>
        </If>
        <If when={removalProgress()}>
          <Text style={{ fontFamily: mono, color: C.muted, fontSize: 10 }}>
            {removalStatus()}
          </Text>
        </If>
        <If when={!scanProgress() && !removalProgress()}>
          <Text style={{ fontFamily: mono, fontSize: 10, color: C.muted }}>
            Pinch / ⌘± Zoom ⌘0 Fit Click selected Enter Space Mark ⌥↑ Parent ⌘R
            Rescan Esc Close review
          </Text>
          <Text
            style={{
              numberOfLines: 1,
              fontFamily: mono,
              flex: 1,
              fontSize: 10,
              color: "#85858f",
              textAlign: "right",
            }}
          >
            {message()}
          </Text>
        </If>
      </View>
    </View>
  );
}
