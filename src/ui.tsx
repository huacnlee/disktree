import { MenuItem } from "nsh/base";
import {
  cleanup,
  effect,
  Text,
  View,
  createAnimatedValue,
  platform,
  reduceMotion,
  Path2D,
  Canvas,
} from "nsh";
import { formatSize, tileColor, type Tile } from "./disk";

export const mono = platform.os === "macos" ? "Menlo" : "monospace";
export const palette = {
  bg: "#070708",
  panel: "#101012",
  line: "#29292d",
  text: "#ededf0",
  muted: "#85858f",
  amber: "#65f193",
};
export const finish = { boxShadow: "inset 0 1px 0 rgba(255,255,255,0.045)" };
/** Static native stroke: the green capacity ring meeting warm storage markers. */
export function AccentBorder(p: { radius: number }) {
  return (
    <Canvas
      style={{
        pointerEvents: "none",
        position: "absolute",
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
      }}
      draw={(ctx, size) => {
        const line = 1,
          inset = line / 2;
        const gradient = ctx.createLinearGradient(
          0,
          size.height,
          size.width,
          0,
        );
        gradient.addColorStop(0, "#327d57");
        gradient.addColorStop(0.42, "#65f193");
        gradient.addColorStop(1, "#d6c28c");
        ctx.strokeStyle = gradient;
        ctx.lineWidth = line;
        ctx.beginPath();
        ctx.roundRect(
          inset,
          inset,
          Math.max(0, size.width - line),
          Math.max(0, size.height - line),
          p.radius,
        );
        ctx.stroke();
      }}
    />
  );
}
export function Option(p: {
  label: string;
  checked?: boolean;
  disabled?: boolean;
  action: () => void;
}) {
  return (
    <MenuItem
      accessibilityLabel={p.label}
      checked={p.checked}
      disabled={p.disabled}
      onSelect={p.action}
      style={{ paddingVertical: 6, paddingHorizontal: 10, borderRadius: 4 }}
      hoverStyle={{ backgroundColor: "#28282d" }}
    >
      <Text
        style={{
          fontFamily: mono,
          fontSize: 12,
          color: p.checked ? palette.amber : palette.text,
        }}
      >
        {p.label}
      </Text>
    </MenuItem>
  );
}
export function Button(p: {
  label: string;
  action?: () => void;
  active?: boolean;
  disabled?: boolean;
  danger?: boolean;
  icon?: "scan" | "parent" | "edit" | "options";
  accessibilityLabel?: string;
}) {
  const press = createAnimatedValue(1);
  const set = (v: number) => {
    if (reduceMotion()) press.set(v);
    else void press.spring(v, { stiffness: 420, damping: 24 });
  };
  return (
    <View
      role="button"
      accessibilityLabel={p.accessibilityLabel || p.label}
      accessibilityState={{ disabled: !!p.disabled }}
      onPress={() => {
        if (!p.disabled) p.action?.();
      }}
      onPressDown={() => {
        if (!p.disabled) set(0.955);
      }}
      onPressUp={() => set(1)}
      hoverStyle={{ backgroundColor: p.icon ? "#1f1f24" : "#222227" }}
      activeStyle={{ backgroundColor: "#2a2a30" }}
      style={{
        width: p.icon ? 32 : undefined,
        height: p.icon ? 32 : undefined,
        paddingHorizontal: p.icon ? 0 : 13,
        paddingVertical: p.icon ? 0 : 10,
        alignItems: "center",
        justifyContent: "center",
        borderRadius: 7,
        borderCurve: "continuous",
        borderWidth: p.icon ? 0 : 1,
        borderColor: p.danger
          ? "#a65a50"
          : p.active && !p.disabled
            ? "#68b985"
            : "#36363d",
        backgroundColor: p.icon
          ? "transparent"
          : p.danger
            ? "#38201e"
            : p.active
              ? "#14251c"
              : "#18181b",
        boxShadow: p.icon
          ? undefined
          : "inset 0 1px 0 rgba(255,255,255,0.06), inset 0 -1px 0 rgba(0,0,0,0.25)",
        opacity: p.disabled ? 0.45 : 1,
        cursor: p.disabled ? "default" : "pointer",
        transform: [{ scale: press }],
      }}
    >
      <If when={!!p.icon}>
        <Canvas
          style={{ pointerEvents: "none", width: 16, height: 16 }}
          draw={(ctx) => {
            const paths = {
              scan: "M20 11A8.1 8.1 0 0 0 4.5 9M4 5v4h4M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4",
              parent:
                "M12 19H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h4l3 3h7a2 2 0 0 1 2 2v3.5M19 22v-6M22 19l-3-3-3 3",
              options:
                "M14 6a2 2 0 1 0 4 0a2 2 0 1 0-4 0M4 6h10M18 6h2M4 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0M8 12h12M11 18a2 2 0 1 0 4 0a2 2 0 1 0-4 0M4 18h7M15 18h5",
              edit: "M4 20h4L18.5 9.5a2.828 2.828 0 1 0-4-4L4 16v4M13.5 6.5l4 4",
            };
            ctx.scale(16 / 24, 16 / 24);
            ctx.strokeStyle = p.icon === "scan" ? palette.amber : "#c4c4cc";
            ctx.lineWidth = 1.75;
            ctx.lineCap = "round";
            ctx.lineJoin = "round";
            ctx.stroke(new Path2D(paths[p.icon!]));
          }}
        />
      </If>
      <If when={!p.icon}>
        <Text
          style={{
            fontFamily: mono,
            fontSize: 12,
            fontWeight: "600",
            color: p.danger ? "#ffc6bd" : p.active ? "#fff" : palette.text,
          }}
        >
          {p.label}
        </Text>
      </If>
    </View>
  );
}
export function MapTile(p: {
  tile: Tile;
  selected: boolean;
  dimmed: boolean;
  marked: boolean;
  metric: string;
  levels: number;
  zoom: number;
  hover: () => void;
  leave: () => void;
  choose: () => void;
  open: () => void;
  context: (event: import("nsh").ContextMenuEvent) => void;
}) {
  cleanup(() => p.leave());
  const ink = () => (p.marked ? "#ffe6ab" : "#e2e5ec");
  const tint = (alpha: number) => {
    const hex = tileColor(p.tile.node, p.tile.depth);
    return `rgba(${parseInt(hex.slice(1, 3), 16)},${parseInt(hex.slice(3, 5), 16)},${parseInt(hex.slice(5, 7), 16)},${alpha})`;
  };
  return (
    <View
      role="button"
      accessibilityLabel={p.tile.node.name}
      onMouseEnter={() => p.hover()}
      onMouseLeave={() => p.leave()}
      onContextMenu={p.context}
      onPress={p.choose}
      class="hover:brightness-125"
      style={{
        position: "absolute",
        left: p.tile.x + 1 / p.zoom,
        top: p.tile.y + 1 / p.zoom,
        width: Math.max(0, p.tile.width - 2 / p.zoom),
        height: Math.max(0, p.tile.height - 2 / p.zoom),
        borderRadius: 3,
        borderCurve: "continuous",
        overflow: "hidden",
        backgroundColor: p.marked ? "rgba(210,145,35,0.2)" : tint(0.09),
        backgroundImage: `linear-gradient(155deg, ${p.marked ? "rgba(255,208,105,0.08)" : tint(0.07)}, rgba(0,0,0,0.12))`,
        borderWidth: (p.marked ? 2 : 1) / p.zoom,
        borderColor: p.marked ? "#ffd36c" : tint(p.selected ? 0.95 : 0.38),
        padding: 5 / p.zoom,
        opacity: p.dimmed ? 0.94 : 1,
        cursor: "pointer",
      }}
    >
      <If when={p.marked}>
        <Canvas
          style={{
            pointerEvents: "none",
            position: "absolute",
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
          }}
          draw={(ctx, size) => {
            ctx.strokeStyle = "rgba(255,221,142,0.25)";
            ctx.lineWidth = 1 / p.zoom;
            ctx.beginPath();
            for (let x = -size.height; x < size.width; x += 10 / p.zoom) {
              ctx.moveTo(x, size.height);
              ctx.lineTo(x + size.height, 0);
            }
            ctx.stroke();
          }}
        />
      </If>
      <If
        when={
          p.marked && p.tile.width * p.zoom > 110 && p.tile.height * p.zoom > 50
        }
      >
        <View
          style={{
            pointerEvents: "none",
            position: "absolute",
            right: 5 / p.zoom,
            bottom: 5 / p.zoom,
            paddingHorizontal: 5 / p.zoom,
            paddingVertical: 2 / p.zoom,
            borderRadius: 3 / p.zoom,
            backgroundColor: "#ffd36c",
            opacity: 1,
          }}
        >
          <Text
            style={{
              fontFamily: mono,
              fontSize: 9 / p.zoom,
              fontWeight: "700",
              color: "#382305",
            }}
          >
            ✓ MARKED
          </Text>
        </View>
      </If>
      <If when={p.tile.width * p.zoom > 70 && p.tile.height * p.zoom > 26}>
        <View style={{ flexDirection: "row", gap: 5, alignItems: "center" }}>
          <Text
            style={{
              numberOfLines: 1,
              fontFamily: mono,
              flex: 1,
              fontSize: 11 / p.zoom,
              fontWeight: "600",
              color: ink(),
            }}
          >
            {p.marked ? "✓ " : ""}
            {p.tile.node.name}
          </Text>
          <If when={p.tile.node.directory && p.tile.width * p.zoom > 140}>
            <Text
              onPress={p.open}
              style={{ fontFamily: mono, fontSize: 13 / p.zoom, color: ink() }}
            >
              ↗
            </Text>
          </If>
        </View>
      </If>
      <If
        when={
          p.tile.width * p.zoom > 90 &&
          p.tile.height * p.zoom > 70 &&
          (p.tile.depth === p.levels || !p.tile.node.children.length)
        }
      >
        <Text
          style={{
            fontFamily: mono,
            marginTop: 5 / p.zoom,
            fontSize: (p.tile.width * p.zoom > 200 ? 22 : 13) / p.zoom,
            fontWeight: "500",
            fontVariant: ["tabular-nums"],
            letterSpacing: -0.5,
            color: ink(),
          }}
        >
          {p.metric === "files"
            ? `${p.tile.node.files} files`
            : formatSize(
                p.metric === "apparent"
                  ? p.tile.node.apparent
                  : p.tile.node.size,
              )}
        </Text>
      </If>
    </View>
  );
}
export function EmptyMap(p: {
  progress: import("./disk").ScanProgress | null;
  seconds: number;
}) {
  return (
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
      <If when={p.progress}>
        <View style={{ width: 360, gap: 14 }}>
          <Text
            style={{ fontFamily: mono, fontSize: 10, color: palette.muted }}
          >
            MAPPING YOUR SPACE
          </Text>
          <Text
            style={{
              fontFamily: mono,
              fontSize: 44,
              color: palette.text,
              fontVariant: ["tabular-nums"],
            }}
          >
            {formatSize(p.progress!.bytes)}
          </Text>
          <View style={{ height: 4, backgroundColor: palette.line }}>
            <View
              style={{
                height: 4,
                width: `${p.progress!.percent}%`,
                backgroundColor: palette.amber,
              }}
            />
          </View>
          <Text
            style={{ fontFamily: mono, fontSize: 12, color: palette.muted }}
          >
            {p.progress!.directories} folders · {p.progress!.files} files ·{" "}
            {p.seconds}s elapsed
          </Text>
          <If when={p.progress!.errors > 0}>
            <Text style={{ color: palette.muted, fontSize: 11 }}>
              {p.progress!.errors} unreadable, skipped
            </Text>
          </If>
        </View>
      </If>
      <If when={!p.progress}>
        <Text style={{ fontFamily: mono, fontSize: 24, color: palette.text }}>
          See the shape of your storage.
        </Text>
        <Text style={{ fontFamily: mono, fontSize: 12, color: palette.muted }}>
          Scan a folder. Find the weight. Make room.
        </Text>
      </If>
    </View>
  );
}
export function MetricControl(p: {
  value: "size" | "apparent" | "files";
  change: (value: "size" | "apparent" | "files") => void;
}) {
  const offset = createAnimatedValue(0);
  const options = [
    { key: "size" as const, label: "Disk size" },
    { key: "apparent" as const, label: "Apparent" },
    { key: "files" as const, label: "Files" },
  ];
  effect(() => {
    const target = options.findIndex((v) => v.key === p.value) * 76;
    if (reduceMotion()) offset.set(target);
    else void offset.spring(target, { stiffness: 350, damping: 32 });
  });
  return (
    <View
      style={{
        flexDirection: "row",
        padding: 3,
        borderRadius: 8,
        backgroundColor: "#090c0a",
        borderWidth: 1,
        borderColor: "transparent",
      }}
    >
      <View
        style={{
          pointerEvents: "none",
          position: "absolute",
          left: 3,
          top: 3,
          bottom: 3,
          width: 76,
          borderRadius: 5,
          backgroundColor: "#111a14",
          borderWidth: 1,
          borderColor: "#68b985",
          transform: [{ translateX: offset }],
        }}
      />
      <Each of={options}>
        {(v) => (
          <View
            role="tab"
            accessibilityLabel={v().label}
            accessibilityState={{ selected: p.value === v().key }}
            onPress={() => p.change(v().key)}
            style={{
              width: 76,
              paddingVertical: 8,
              borderRadius: 5,
              borderWidth: 1,
              borderColor: "transparent",
              alignItems: "center",
              cursor: "pointer",
            }}
          >
            <Text
              style={{
                fontFamily: mono,
                fontSize: 11,
                fontWeight: "500",
                color: p.value === v().key ? "#f1f1f4" : "#939b95",
              }}
            >
              {v().label}
            </Text>
          </View>
        )}
      </Each>
    </View>
  );
}

/** Original capacity-ring mark; shared geometry with assets/icon.svg. */
export function AppMark() {
  return (
    <Canvas
      style={{ pointerEvents: "none", width: 24, height: 24 }}
      draw={(ctx) => {
        ctx.fillStyle = "#101012";
        ctx.beginPath();
        ctx.roundRect(0, 0, 24, 24, 6);
        ctx.fill();
        ctx.strokeStyle = "#65f193";
        ctx.lineWidth = 1.8;
        ctx.lineCap = "round";
        ctx.beginPath();
        ctx.arc(12, 12, 6.6, -0.7, Math.PI * 2);
        ctx.stroke();
        ctx.strokeStyle = "#baffcb";
        ctx.lineWidth = 1.1;
        ctx.beginPath();
        ctx.moveTo(9.4, 9.4);
        ctx.lineTo(13.8, 9.4);
        ctx.moveTo(9.4, 12);
        ctx.lineTo(15.2, 12);
        ctx.moveTo(9.4, 14.6);
        ctx.lineTo(12.6, 14.6);
        ctx.stroke();
        ctx.fillStyle = "#baffcb";
        ctx.beginPath();
        ctx.arc(18.6, 8.5, 1, 0, Math.PI * 2);
        ctx.fill();
      }}
    />
  );
}
