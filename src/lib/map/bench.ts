import type { Names } from "$lib/data/names";
import type { Graph } from "@cosmos.gl/graph";

import { glRenderer, watchFrames } from "./debug";
import type { MapState } from "./state.svelte";

export type Mark = { kind: string; ms: number; at: number; detail: Record<string, number> };
type Step = { label: string; start: number; end: number; marks: Mark[] };

let current: Step | null = null;
const steps: Step[] = [];
const frameTimes: number[] = [];
const longTasks: { at: number; ms: number }[] = [];
const RECENT_MARKS = 200;
const recent: Mark[] = [];

export type BenchEvent =
  | { type: "step"; label: string }
  | { type: "mark"; mark: Mark }
  | { type: "result"; step: StepReport }
  | {
      type: "end";
      steps: StepReport[];
      wentHidden: boolean;
      stopped: string | null;
      table: string;
      json: string;
    };
let onEvent: ((event: BenchEvent) => void) | null = null;
let stop: AbortSignal | null = null;
class BenchStopped extends Error {}

export const mark = (kind: string, ms: number, detail: Record<string, number> = {}): void => {
  const m: Mark = { kind, ms, at: performance.now(), detail };
  if (current) {
    current.marks.push(m);
    onEvent?.({ type: "mark", mark: m });
  }
  recent.push(m);
  if (recent.length > RECENT_MARKS) recent.shift();
};

export const workDuring = (start: number, end: number): string => {
  const hits = recent
    .filter((m) => m.at > start && m.at - m.ms < end)
    .sort((a, b) => b.ms - a.ms)
    .slice(0, 3);
  if (hits.length === 0) return "no recorded work";
  return hits
    .map((m) => {
      const [key, value] = Object.entries(m.detail)[0] ?? [];
      const what = key === undefined ? "" : ` (${value.toLocaleString()} ${key})`;
      return `${m.kind} ${Math.round(m.ms)}ms${what}`;
    })
    .join(", ");
};

const r2 = (v: number) => Math.round(v * 100) / 100;
const quantile = (sorted: number[], q: number) =>
  sorted.length === 0 ? 0 : sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];

type KindSummary = {
  count: number;
  totalMs: number;
  maxMs: number;
  slowest: Record<string, number>;
};

export type StepReport = {
  label: string;
  ms: number;
  frames: number;
  fps: number;
  p50FrameMs: number;
  p95FrameMs: number;
  maxFrameMs: number;
  over33: number;
  longTasks: number;
  longTaskMs: number;
  marks: Record<string, KindSummary>;
};

const summarize = (s: Step): StepReport => {
  const window = frameTimes.filter((t) => t >= s.start && t <= s.end);
  const gaps: number[] = [];
  for (let i = 1; i < window.length; i++) gaps.push(window[i] - window[i - 1]);
  const sorted = [...gaps].sort((a, b) => a - b);
  const ms = s.end - s.start;
  const marks: Record<string, KindSummary> = {};
  for (const m of s.marks) {
    const k = (marks[m.kind] ??= { count: 0, totalMs: 0, maxMs: 0, slowest: {} });
    k.count++;
    k.totalMs = r2(k.totalMs + m.ms);
    if (m.ms > k.maxMs) {
      k.maxMs = r2(m.ms);
      k.slowest = m.detail;
    }
  }
  const slow = longTasks.filter((l) => l.at >= s.start && l.at <= s.end);
  return {
    label: s.label,
    ms: Math.round(ms),
    frames: window.length,
    fps: r2((window.length * 1000) / ms),
    p50FrameMs: r2(quantile(sorted, 0.5)),
    p95FrameMs: r2(quantile(sorted, 0.95)),
    maxFrameMs: r2(sorted[sorted.length - 1] ?? 0),
    over33: gaps.filter((g) => g > 33.3).length,
    longTasks: slow.length,
    longTaskMs: Math.round(slow.reduce((a, l) => a + l.ms, 0)),
    marks
  };
};

const gpuTimerExtension = (canvas: HTMLCanvasElement | null): string | null => {
  const gl = canvas?.getContext("webgl2");
  if (!gl) return null;
  for (const name of ["EXT_disjoint_timer_query_webgl2", "EXT_disjoint_timer_query"])
    if (gl.getExtension(name)) return name;
  return null;
};

export type BenchContext = {
  graph: Graph;
  overlay: HTMLCanvasElement;
  ui: MapState;
  positions: Float32Array;
  space: number;
  names: Names;
  years: number[];
  nodes: number;
  edges: number;
  settings: { pixelRatio: number };
};

export type BenchMode = "run" | "profile";

type ProfilerFrame = { name: string; resourceId?: number; line?: number; column?: number };
type ProfilerStack = { frameId: number; parentId?: number };
type ProfilerSample = { timestamp: number; stackId?: number };
type ProfilerTrace = {
  frames: ProfilerFrame[];
  resources: string[];
  stacks: ProfilerStack[];
  samples: ProfilerSample[];
};
type ProfilerHandle = { stop: () => Promise<ProfilerTrace> };
type ProfilerCtor = new (o: { sampleInterval: number; maxBufferSize: number }) => ProfilerHandle;

const profilerCtor = (): ProfilerCtor | undefined =>
  (window as unknown as { Profiler?: ProfilerCtor }).Profiler;

const summarizeProfile = (trace: ProfilerTrace): string => {
  const n = trace.samples.length;
  if (n < 2) return "no samples";
  const span = trace.samples[n - 1].timestamp - trace.samples[0].timestamp;
  const per = span / (n - 1);
  const label = (id: number) => {
    const f = trace.frames[id];
    const res = f.resourceId === undefined ? "" : (trace.resources[f.resourceId] ?? "");
    const file = res.split("/").pop()?.split("?")[0] ?? "";
    return `${f.name || "(anonymous)"}${file ? ` @${file}${f.line ? `:${f.line}` : ""}` : ""}`;
  };
  const self = new Map<number, number>();
  const total = new Map<number, number>();
  for (const sample of trace.samples) {
    if (sample.stackId === undefined) continue;
    let sid: number | undefined = sample.stackId;
    const seen = new Set<number>();
    let leaf = true;
    while (sid !== undefined) {
      const stack: ProfilerStack = trace.stacks[sid];
      if (leaf) self.set(stack.frameId, (self.get(stack.frameId) ?? 0) + 1);
      leaf = false;
      if (!seen.has(stack.frameId)) {
        seen.add(stack.frameId);
        total.set(stack.frameId, (total.get(stack.frameId) ?? 0) + 1);
      }
      sid = stack.parentId;
    }
  }
  const rows = [...self.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 14)
    .map(([id, count]) => {
      const ms = (count * per).toFixed(0);
      const tot = ((total.get(id) ?? 0) * per).toFixed(0);
      return `${ms.padStart(7)}${tot.padStart(9)}  ${label(id)}`;
    });
  const idle = trace.samples.filter((x) => x.stackId === undefined).length;
  return [
    `${n} samples over ${span.toFixed(0)}ms (${per.toFixed(2)}ms apart), ${(idle * per).toFixed(0)}ms idle`,
    "   self    total  frame",
    ...rows
  ].join("\n");
};

const profiled = async (label: string, body: () => Promise<void>): Promise<void> => {
  const Ctor = profilerCtor();
  if (!Ctor) {
    console.warn("no js self-profiling: the page needs Document-Policy: js-profiling");
    await body();
    return;
  }
  const profiler = new Ctor({ sampleInterval: 1, maxBufferSize: 900_000 });
  await body();
  const trace = await profiler.stop();
  if (!stop?.aborted) console.log(`profile: ${label}\n${summarizeProfile(trace)}`);
};

const PAN_ZOOM = 4;

const whenVisible = (): Promise<void> =>
  document.visibilityState === "visible"
    ? Promise.resolve()
    : new Promise((resolve) => {
        console.log("bench waiting: this tab has to be visible to get frames");
        const done = () => {
          document.removeEventListener("visibilitychange", onChange);
          stop?.removeEventListener("abort", done);
          resolve();
        };
        const onChange = () => {
          if (document.visibilityState === "visible") done();
        };
        document.addEventListener("visibilitychange", onChange);
        stop?.addEventListener("abort", done);
      });

const sleep = (ms: number) =>
  new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer);
      stop?.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    stop?.addEventListener("abort", done);
  });

const quiesce = async (quietMs = 800, limitMs = 8000): Promise<void> => {
  const probe: Step = { label: "quiesce", start: performance.now(), end: 0, marks: [] };
  current = probe;
  onEvent?.({ type: "step", label: "settling" });
  const deadline = performance.now() + limitMs;
  let seen = 0;
  let quietSince = performance.now();
  while (performance.now() < deadline && !stop?.aborted) {
    await sleep(100);
    if (probe.marks.length !== seen) {
      seen = probe.marks.length;
      quietSince = performance.now();
    } else if (performance.now() - quietSince >= quietMs) break;
  }
  current = null;
};

const step = async (label: string, body: () => Promise<void>): Promise<void> => {
  if (stop?.aborted) throw new BenchStopped(label);
  const s: Step = { label, start: performance.now(), end: 0, marks: [] };
  current = s;
  onEvent?.({ type: "step", label });
  await body();
  current = null;
  if (stop?.aborted) throw new BenchStopped(label);
  s.end = performance.now();
  steps.push(s);
  onEvent?.({ type: "result", step: summarize(s) });
};

const table = (rows: StepReport[]): string => {
  const cols: [string, (r: StepReport) => string][] = [
    ["step", (r) => r.label],
    ["ms", (r) => String(r.ms)],
    ["fps", (r) => r.fps.toFixed(1)],
    ["p50", (r) => r.p50FrameMs.toFixed(1)],
    ["p95", (r) => r.p95FrameMs.toFixed(1)],
    ["max", (r) => r.maxFrameMs.toFixed(1)],
    [">33", (r) => String(r.over33)],
    ["blocked", (r) => String(r.longTaskMs)],
    ["links", (r) => String(r.marks["links.upload"]?.slowest.links ?? "")],
    ["worker", (r) => (r.marks["worker.detail"]?.maxMs ?? 0).toFixed(1)],
    ["overlay", (r) => (r.marks.overlay?.maxMs ?? 0).toFixed(1)]
  ];
  const widths = cols.map(([head, get]) =>
    Math.max(head.length, ...rows.map((r) => get(r).length))
  );
  const line = (cells: string[]) =>
    cells.map((c, i) => (i === 0 ? c.padEnd(widths[i]) : c.padStart(widths[i]))).join("  ");
  return [
    line(cols.map(([head]) => head)),
    line(widths.map((w) => "-".repeat(w))),
    ...rows.map((r) => line(cols.map(([, get]) => get(r))))
  ].join("\n");
};

export const runBench = async (
  ctx: BenchContext,
  mode: BenchMode,
  report: (event: BenchEvent) => void,
  signal: AbortSignal
): Promise<void> => {
  onEvent = report;
  stop = signal;
  steps.length = 0;
  frameTimes.length = 0;
  longTasks.length = 0;
  const { graph: g, overlay, ui } = ctx;
  const w = overlay.clientWidth;
  const h = overlay.clientHeight;
  const fitZoom = Math.min(w, h) / ctx.space;
  const center = ctx.space / 2;
  const hubX = ctx.positions[0];
  const hubY = ctx.positions[1];

  onEvent?.({ type: "step", label: "waiting for the tab" });
  await whenVisible();
  let wentHidden = false;
  const onHide = () => {
    if (document.visibilityState !== "visible") wentHidden = true;
  };
  document.addEventListener("visibilitychange", onHide);
  const stopFrames = watchFrames((now) => frameTimes.push(now));
  const observer = new PerformanceObserver((list) => {
    for (const e of list.getEntries()) longTasks.push({ at: e.startTime, ms: e.duration });
  });
  observer.observe({ entryTypes: ["longtask"] });
  await quiesce();

  const fly = (x: number, y: number, zoom: number, ms: number) =>
    g.setZoomTransformByPointPositions(new Float32Array([x, y]), ms, zoom, undefined, false);

  const move = (label: string, x: number, y: number, zoom: number) =>
    step(label, async () => {
      fly(x, y, zoom, 700);
      await sleep(2500);
    });

  let stopped: string | null = null;
  try {
    if (mode === "profile") {
      fly(center, center, fitZoom, 0);
      await step("profile zoom in", () =>
        profiled("zooming in, which rewrites the detail run", async () => {
          fly(hubX, hubY, PAN_ZOOM, 700);
          await sleep(3500);
        })
      );
      await step("profile select hub", () =>
        profiled("selecting the highest-count tag", async () => {
          ui.selected = 0;
          await sleep(3000);
        })
      );
      await step("profile deselect", () =>
        profiled("clearing the selection", async () => {
          ui.selected = null;
          await sleep(3000);
        })
      );
      await step("profile year step", () =>
        profiled("one year step of the scrub", async () => {
          ui.yearRange = [ctx.years[0], ctx.years[ctx.years.length - 3]];
          await sleep(3000);
        })
      );
    } else {
      await step("idle", () => sleep(1500));
      await move("overview", center, center, fitZoom);
      await move("zoom-1", hubX, hubY, 1);
      await move("zoom-2", hubX, hubY, PAN_ZOOM);
      await step("pan", async () => {
        const d = (w * 0.6) / PAN_ZOOM;
        let x = hubX;
        let y = hubY;
        for (const [dx, dy] of [
          [d, 0],
          [0, d],
          [-d, 0],
          [0, -d]
        ]) {
          x += dx;
          y += dy;
          fly(x, y, PAN_ZOOM, 400);
          await sleep(900);
        }
      });
      await move("zoom-3", hubX, hubY, 20);
      await move("zoom-out", center, center, fitZoom);
      await step("select-hub", async () => {
        ui.selected = 0;
        await sleep(2500);
      });
      await step("deselect", async () => {
        ui.selected = null;
        await sleep(1200);
      });
      await step("filter-off", async () => {
        ui.categoriesOn[1] = false;
        await sleep(1500);
      });
      await step("filter-on", async () => {
        ui.categoriesOn[1] = true;
        await sleep(1500);
      });
      await step("year-scrub", async () => {
        for (const year of ctx.years.slice(-11)) {
          ui.yearRange = [ctx.years[0], year];
          await sleep(350);
        }
      });
      await step("year-all", async () => {
        ui.yearRange = null;
        await sleep(1500);
      });
    }
  } catch (e) {
    if (!(e instanceof BenchStopped)) throw e;
    stopped = e.message;
    ui.selected = null;
    ui.yearRange = null;
    ui.categoriesOn[1] = true;
  } finally {
    stopFrames();
    observer.disconnect();
    document.removeEventListener("visibilitychange", onHide);
    stop = null;
  }

  const rows = steps.map(summarize);
  const result = {
    when: new Date().toISOString(),
    renderer: glRenderer(),
    gpuTimerExtension: gpuTimerExtension(overlay.parentElement?.querySelector("canvas") ?? null),
    userAgent: navigator.userAgent,
    viewport: { width: w, height: h, devicePixelRatio: window.devicePixelRatio },
    dataset: { nodes: ctx.nodes, edges: ctx.edges, hub: ctx.names.at(0) },
    settings: ctx.settings,
    wentHidden,
    stopped,
    steps: rows
  };
  Object.assign(window, { atlas621bench: result });
  if (wentHidden) console.warn("the tab was hidden during this run; the frame numbers are junk");
  const json = JSON.stringify(result);
  console.log(table(rows));
  console.log(`bench json follows; also on window.atlas621bench\n${json}`);
  onEvent?.({ type: "end", steps: rows, wentHidden, stopped, table: table(rows), json });
  onEvent = null;
};
