<script module lang="ts">
  import { cssToken } from "$lib/core/css";
  import type { LabelZooms } from "$lib/core/declutter";
  import type { Names } from "$lib/core/strings";
  import {
    loadBase,
    loadCommunities,
    loadCore,
    loadImplications,
    loadManifest,
    loadNames,
    loadTerritories,
    loadU16,
    loadYears,
    type Communities,
    type Core,
    type SearchEntry,
    type Territories,
    type Years
  } from "$lib/data/dataset";
  import type { Manifest } from "$lib/data/manifest";
  import {
    beginStep,
    paint,
    resetProgress,
    STEPS,
    watchProgress,
    type Progress
  } from "$lib/data/progress";

  import { mark } from "./bench";
  import { loadLabelZooms } from "./labelzooms";
  import { ZOOM_MAX } from "./MapCanvas.svelte";

  type LabelFeed = { latest: LabelZooms | null; onchange: ((zooms: LabelZooms) => void) | null };

  type Dataset = {
    core: Core;
    names: Names;
    firstYear: Uint16Array;
    territories: Territories;
    communities: Communities;
    links: Worker;
    labels: LabelFeed;
    labelsDone: Promise<LabelZooms>;
  };

  let cached: Promise<Dataset> | null = null;
  let cachedYears: Promise<Years> | null = null;

  const loadDataset = async (): Promise<Dataset> => {
    resetProgress();
    const manifest = await loadManifest();
    const n = manifest.nodes;
    const coreLoading = loadCore(manifest);
    const namesLoading = loadNames(manifest);
    const labels: LabelFeed = { latest: null, onchange: null };
    const labelsDone = Promise.all([coreLoading, namesLoading]).then(async ([core, names]) => {
      const t0 = performance.now();
      const zooms = await loadLabelZooms(core, names, cssToken("--font-sans"), ZOOM_MAX, (z) => {
        labels.latest = z;
        labels.onchange?.(z);
      });
      mark("labels.zooms", performance.now() - t0, { nodes: zooms.zoom.length });
      return zooms;
    });
    const [core, names, firstYear, territories, communities, base, implications] =
      await Promise.all([
        coreLoading,
        namesLoading,
        loadU16(manifest, "first_year.bin.gz", "first_year", "first years"),
        loadTerritories(manifest),
        loadCommunities(manifest),
        loadBase(manifest),
        loadImplications(manifest)
      ]);
    const links = new LinksWorker();
    const init: LinksInit = {
      type: "init",
      data: {
        positions: core.positions,
        postCounts: core.postCounts,
        base,
        implications,
        edgeCount: manifest.edges,
        maxDegree: manifest.max_degree,
        space: manifest.space_size,
        tileCutoffs: manifest.tile_cutoffs,
        tiles: Object.keys(manifest.files).filter((f) => f.startsWith("tiles/")),
        adjShardSize: manifest.adj_shard_size,
        version: manifest.generated_at
      }
    };
    links.postMessage(init, [
      ...new Set([base.index.buffer, base.a.buffer, base.b.buffer, base.weight.buffer])
    ]);
    return { core, names, firstYear, territories, communities, links, labels, labelsDone };
  };
</script>

<script lang="ts">
  import { dev } from "$app/environment";
  import { goto, replaceState } from "$app/navigation";
  import { resolve } from "$app/paths";
  import { page } from "$app/state";
  import { glRenderer, startFpsMeter, startHitchWatch, type Hitch } from "$lib/core/frames";
  import { tagPath } from "$lib/tagpath";
  import Nav from "$lib/ui/Nav.svelte";
  import Settings from "@lucide/svelte/icons/settings";
  import X from "@lucide/svelte/icons/x";
  import NumberFlow, { NumberFlowGroup } from "@number-flow/svelte";
  import { Collapsible } from "bits-ui";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";

  import { workDuring, type BenchMode, type Mark, type StepReport } from "./bench";
  import Controls from "./Controls.svelte";
  import DetailPanel from "./DetailPanel.svelte";
  import LinksWorker from "./links.worker?worker";
  import type { LinksInit } from "./links.worker";
  import MapCanvas from "./MapCanvas.svelte";
  import SearchBox from "./SearchBox.svelte";
  import { getMapState } from "./state.svelte";

  const ui = getMapState();

  let data = $state.raw<Dataset | null>(null);
  const core = $derived(data?.core ?? null);
  const names = $derived(data?.names ?? null);
  let years = $state.raw<Years | null>(null);
  let labelZooms = $state.raw<LabelZooms | null>(null);
  let error = $state<string | null>(null);
  let ready = $state(false);
  let progress = $state<Progress>({ tasks: [] });
  let latestProgress: Progress = { tasks: [] };
  let progressFrame = 0;
  watchProgress((p) => {
    latestProgress = p;
    if (progressFrame) return;
    progressFrame = requestAnimationFrame(() => {
      progressFrame = 0;
      progress = latestProgress;
    });
  });
  $effect(() => () => {
    watchProgress(null);
    cancelAnimationFrame(progressFrame);
  });
  const mb = (bytes: number) => (bytes / 1e6).toFixed(1);
  const fileTasks = $derived(progress.tasks.filter((t) => t.kind === "bytes"));
  const loaded = $derived(fileTasks.reduce((sum, t) => sum + Math.min(t.loaded, t.expected), 0));
  const expected = $derived(fileTasks.reduce((sum, t) => sum + t.expected, 0));
  const filesDone = $derived(fileTasks.filter((t) => t.done).length);
  const BYTES_SHARE = 0.7;
  const barFraction = $derived.by(() => {
    const bytes = expected ? loaded / expected : 0;
    const stepsDone = STEPS.filter((s) => progress.tasks.some((t) => t.label === s && t.done));
    return BYTES_SHARE * bytes + (1 - BYTES_SHARE) * (stepsDone.length / STEPS.length);
  });
  const status = $derived.by(() => {
    const active = progress.tasks.filter((t) => !t.done);
    const parts: string[] = [];
    for (const phase of ["downloading", "inflating", "parsing"] as const) {
      const labels = active.filter((t) => t.phase === phase).map((t) => t.label);
      if (labels.length > 2) parts.push(`${phase} ${labels.length} files`);
      else if (labels.length) parts.push(`${phase} ${labels.join(" and ")}`);
    }
    for (const t of active) if (t.kind === "step") parts.push(t.label);
    return parts.join(" · ");
  });
  let searchBox = $state<ReturnType<typeof SearchBox>>();
  let focus = $state.raw<{ node: number; seq: number } | null>(null);
  let controlsOpen = $state(true);
  let drawnLinks = $state(0);
  let fps = $state(0);
  let canvas = $state<ReturnType<typeof MapCanvas>>();
  const BENCH_MODES: BenchMode[] = ["run", "profile"];
  type BenchPanel = {
    mode: BenchMode;
    startedAt: number;
    lines: string[];
    tail: { kind: string; count: number; max: Mark } | null;
    running: boolean;
    json: string | null;
  };
  let bench = $state.raw<BenchPanel | null>(null);
  let benchStop: AbortController | null = null;
  let benchOpen = $state(false);
  const ms1 = (v: number) => v.toFixed(1);
  const markText = (m: Mark) => {
    const [key, value] = Object.entries(m.detail)[0] ?? [];
    return `${m.kind} ${ms1(m.ms)}ms${key === undefined ? "" : ` (${value.toLocaleString()} ${key})`}`;
  };
  const resultText = (r: StepReport) =>
    `= ${r.label.padEnd(12)} ${ms1(r.fps).padStart(5)} fps  p50 ${ms1(r.p50FrameMs).padStart(5)}  p95 ${ms1(r.p95FrameMs).padStart(5)}  max ${ms1(r.maxFrameMs).padStart(6)}  >33 ${String(r.over33).padStart(2)}  blocked ${String(r.longTaskMs).padStart(4)}ms`;
  const startBench = (mode: BenchMode) => {
    const renderer = glRenderer().replace(/ \(.*$/, "");
    bench = {
      mode,
      startedAt: performance.now(),
      lines: [
        `bench ${mode} · ${renderer} · ${window.innerWidth}×${window.innerHeight} at ${window.devicePixelRatio.toFixed(2)}`
      ],
      tail: null,
      running: true,
      json: null
    };
    benchStop = new AbortController();
    const run = canvas?.bench(
      mode,
      (event) =>
        untrack(() => {
          if (!bench) return;
          const lines = [...bench.lines];
          let tail = bench.tail;
          if (event.type === "mark") {
            const m = event.mark;
            if (tail && tail.kind === m.kind) {
              tail = {
                kind: m.kind,
                count: tail.count + 1,
                max: m.ms > tail.max.ms ? m : tail.max
              };
              lines[lines.length - 1] = `  ${markText(tail.max)} ×${tail.count}`;
            } else {
              tail = { kind: m.kind, count: 1, max: m };
              lines.push(`  ${markText(m)}`);
            }
            bench = { ...bench, lines, tail };
            return;
          }
          tail = null;
          if (event.type === "step") lines.push(`> ${event.label}`);
          else if (event.type === "result") lines.push(resultText(event.step));
          else {
            const seconds = ((performance.now() - bench.startedAt) / 1000).toFixed(1);
            lines.push(
              `${event.stopped === null ? "done in" : `stopped in ${event.stopped} after`} ${seconds} s${event.wentHidden ? ", but the tab was hidden during the run; the frame numbers are junk" : ""}`,
              "",
              event.table,
              "",
              "json on window.atlas621bench"
            );
            bench = { ...bench, lines, tail, running: false, json: event.json };
            return;
          }
          bench = { ...bench, lines, tail };
        }),
      benchStop.signal
    );
    run?.catch((e: unknown) => {
      if (bench)
        bench = { ...bench, lines: [...bench.lines, `error: ${String(e)}`], running: false };
    });
  };
  const followTail: Attachment<HTMLPreElement> = (pre) => {
    if (bench) pre.scrollTop = pre.scrollHeight;
  };
  let copied = $state<string | null>(null);
  const copy = async (what: string, text: string) => {
    await navigator.clipboard.writeText(text);
    copied = what;
    setTimeout(() => (copied = null), 1200);
  };
  let lastHitch = $state<string | null>(null);
  const hitches: (Hitch & { work: string })[] = [];
  const onHitch = (h: Hitch) => {
    const work = workDuring(h.at, h.at + h.ms);
    hitches.push({ ...h, work });
    lastHitch = `${Math.round(h.ms)}ms · ${work}`;
    console.warn(`hitch ${Math.round(h.ms)}ms · ${work}`);
  };
  $effect(() => {
    if (!dev) return;
    Object.assign(window, { atlas621hitches: hitches });
    const stopFps = startFpsMeter((v) => (fps = v));
    const stopHitches = startHitchWatch(onHitch);
    return () => {
      stopFps();
      stopHitches();
    };
  });

  const parseViewport = (v: string | null) => {
    if (!v) return null;
    const [x, y, zoom] = v.split(",").map(Number);
    return [x, y, zoom].every(Number.isFinite) ? { x, y, zoom } : null;
  };
  const initialViewport = parseViewport(page.url.searchParams.get("v"));

  const failed = (e: unknown) => {
    error = e instanceof Error ? e.message : String(e);
  };
  const ensureYears = (manifest: Manifest) => {
    if (years) return;
    cachedYears ??= loadYears(manifest);
    cachedYears.then(
      (y) => (years = y),
      (e) => {
        cachedYears = null;
        failed(e);
      }
    );
  };
  const load = async () => {
    try {
      cached ??= loadDataset();
      const [loaded] = await Promise.all([cached, document.fonts.ready]);
      beginStep("placing tags");
      await paint();
      data = loaded;
      ensureYears(loaded.core.manifest);
      loaded.labelsDone.catch((e) => {
        cached = null;
        failed(e);
      });
    } catch (e) {
      cached = null;
      failed(e);
    }
  };
  load();
  $effect(() => {
    if (!data) return;
    const feed = data.labels;
    labelZooms = feed.latest;
    feed.onchange = (z) => (labelZooms = z);
    return () => {
      feed.onchange = null;
    };
  });
  $effect(() => {
    if (data && (ui.yearRange !== null || ui.selected !== null)) ensureYears(data.core.manifest);
  });

  $effect(() => {
    const tag = page.params.tag;
    if (!names) return;
    const current = untrack(() => ui.selected);
    if (tag === undefined) {
      if (current !== null) ui.selected = null;
      return;
    }
    const idx = names.indexOf(tag);
    if (idx !== undefined && idx !== current) {
      ui.selected = idx;
      focus = { node: idx, seq: (focus?.seq ?? 0) + 1 };
    }
  });

  const pathFor = (sel: number | null) =>
    sel === null || !names ? resolve("/(map)") : tagPath(names.at(sel));

  $effect(() => {
    const sel = ui.selected;
    if (!names) return;
    const want = pathFor(sel);
    if (page.url.pathname !== want) {
      goto(want + page.url.search, { replaceState: true, keepFocus: true, noScroll: true });
    }
  });

  const flyTo = (node: number) => {
    ui.selected = node;
    focus = { node, seq: (focus?.seq ?? 0) + 1 };
  };
  const followTo = (node: number) => {
    ui.follow(node);
    focus = { node, seq: (focus?.seq ?? 0) + 1 };
  };

  const onkeydown = (event: KeyboardEvent) => {
    const target = event.target;
    if (target instanceof HTMLElement && target.closest("input, textarea, [contenteditable]"))
      return;
    if (event.key === "Escape" && ui.selected !== null) ui.selected = null;
    else if (event.key === "/") {
      event.preventDefault();
      searchBox?.focus();
    }
  };

  const onpick = (entry: SearchEntry) => {
    if (entry.i >= 0) flyTo(entry.i);
    else if (entry.t && entry.t.length > 0) flyTo(entry.t[0]);
  };

  const onviewport = (v: { x: number; y: number; zoom: number }) => {
    const params = new URLSearchParams(page.url.search);
    const next = `${v.x.toFixed(1)},${v.y.toFixed(1)},${v.zoom.toFixed(3)}`;
    if (params.get("v") === next) return;
    params.set("v", next);
    replaceState(`${pathFor(ui.selected)}?${params}`, {});
  };
</script>

<svelte:head>
  <title
    >{ui.selected !== null && names ? `${names.at(ui.selected)} · atlas621` : "atlas621"}</title>
</svelte:head>

<svelte:window {onkeydown} />

<div class="flex h-svh flex-col">
  <Nav current="map">
    {#if core}
      <span class="truncate tabular-nums">
        <NumberFlowGroup>
          e621 tags as of {core.manifest.export_date} ·
          <NumberFlow value={ready ? core.manifest.nodes : 0} /> tags ·
          <NumberFlow value={ready ? drawnLinks : 0} />/<NumberFlow
            value={ready ? core.manifest.edges : 0} /> edges
        </NumberFlowGroup>
      </span>
    {/if}
  </Nav>
  <div class="relative min-h-0 flex-1 overflow-hidden bg-page-dim">
    {#if data}
      <MapCanvas
        bind:this={canvas}
        core={data.core}
        names={data.names}
        links={data.links}
        firstYear={data.firstYear}
        {years}
        territories={data.territories}
        communities={data.communities}
        {labelZooms}
        {focus}
        viewport={initialViewport}
        onready={() => (ready = true)}
        onerror={failed}
        onlinks={(n) => (drawnLinks = n)}
        {onviewport} />
    {/if}
    <div class="pointer-events-none absolute inset-0 p-2">
      <aside
        class="pointer-events-auto w-64 max-w-[calc(100vw-1rem)] rounded-sm bg-page/95 p-2 backdrop-blur">
        <Collapsible.Root bind:open={controlsOpen} class="flex flex-col gap-2">
          <div class="flex items-center justify-between select-none">
            <h1 class="text-base font-bold">tags</h1>
            <Collapsible.Trigger class="btn px-1.5 py-0.5" aria-label="toggle controls">
              <Settings class="size-3.5" />
            </Collapsible.Trigger>
          </div>
          <SearchBox bind:this={searchBox} {core} {names} {onpick} />
          <Collapsible.Content>
            {#if core}
              <Controls years={core.manifest.years} />
            {/if}
          </Collapsible.Content>
        </Collapsible.Root>
      </aside>
      {#if ui.selected !== null && data}
        <div class="pointer-events-auto absolute top-2 right-2 bottom-2 hidden flex-col sm:flex">
          <DetailPanel
            node={ui.selected}
            core={data.core}
            names={data.names}
            {years}
            firstYear={data.firstYear}
            communities={data.communities}
            onnavigate={followTo} />
        </div>
      {/if}
      {#if dev}
        <div class="absolute right-2 bottom-2 text-xs">
          <span
            class="pointer-events-auto flex items-center gap-2 rounded-sm bg-page/90 px-2 py-1 text-right font-mono text-muted">
            <span>
              {fps} fps · {glRenderer()}
              {#if lastHitch}<br /><span class="text-accent">{lastHitch}</span>{/if}
            </span>
            <button type="button" class="btn px-1.5 py-0.5" onclick={() => (benchOpen = !benchOpen)}
              >bench</button>
          </span>
        </div>
        {#if benchOpen}
          {@const log = bench ? bench.lines.join("\n") : "no run yet"}
          <div
            class="pointer-events-auto absolute right-2 bottom-14 flex h-96 max-h-[calc(100svh-6rem)] w-176 max-w-[calc(100vw-1rem)] flex-col gap-1 rounded-sm bg-page/95 p-2 text-xs text-muted backdrop-blur">
            <div class="flex items-center gap-1">
              <span class="mr-1 font-bold text-ink">bench</span>
              {#each BENCH_MODES as mode (mode)}
                <button
                  type="button"
                  class="btn px-1.5 py-0.5"
                  disabled={bench?.running}
                  onclick={() => startBench(mode)}>{mode}</button>
              {/each}
              {#if bench?.running}
                <button type="button" class="btn px-1.5 py-0.5" onclick={() => benchStop?.abort()}
                  >stop</button>
              {/if}
              <span class="mr-auto"></span>
              {#if copied}<span>copied {copied}</span>{/if}
              {#if bench}
                <button type="button" class="btn px-1.5 py-0.5" onclick={() => copy("log", log)}
                  >copy log</button>
              {/if}
              {#if bench?.json}
                {@const json = bench.json}
                <button type="button" class="btn px-1.5 py-0.5" onclick={() => copy("json", json)}
                  >copy json</button>
              {/if}
              <button
                type="button"
                class="btn px-1.5 py-0.5"
                onclick={() => (benchOpen = false)}
                aria-label="close"><X class="size-3" /></button>
            </div>
            <pre
              class="min-h-0 flex-1 overflow-auto rounded-xs bg-ground p-2 font-mono text-ink/90 select-text"
              {@attach followTail}>{log}</pre>
          </div>
        {/if}
      {/if}
    </div>
    {#if ui.selected !== null && data}
      <div class="absolute inset-x-0 bottom-0 flex max-h-[55svh] flex-col p-2 sm:hidden">
        <DetailPanel
          node={ui.selected}
          core={data.core}
          names={data.names}
          {years}
          firstYear={data.firstYear}
          communities={data.communities}
          onnavigate={followTo} />
      </div>
    {/if}
    {#if error}
      <p class="absolute inset-x-0 top-1/2 text-center text-muted">
        could not load the dataset: {error}
      </p>
    {:else if !ready}
      <div
        class="hex-strip absolute inset-0 flex flex-col items-center justify-center gap-2 bg-page text-muted select-none">
        <span>loading the map</span>
        <span class="h-1 w-96 overflow-hidden rounded-xs bg-ground inset-shadow-track">
          <span class="block h-full bg-accent transition-[width]" style:width="{100 * barFraction}%"
          ></span>
        </span>
        <p class="h-8 w-96 text-center font-mono text-xs tabular-nums">{status}</p>
        <span class="font-mono text-xs tabular-nums">
          {mb(loaded)} of {mb(expected)} MB · {filesDone}/{fileTasks.length} files
        </span>
      </div>
    {/if}
  </div>
</div>
