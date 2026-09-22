<script module lang="ts">
  export const ZOOM_MAX = 300;
  const ZOOM_MIN = 0.05;
  const ZOOM_PER_NOTCH = 0.35;
  let sceneId = 0;
  let baseId = 0;
  let detailId = 0;
  let forcedId = 0;
</script>

<script lang="ts">
  import { dev } from "$app/environment";
  import { cssToken } from "$lib/core/css";
  import type { LabelZooms } from "$lib/core/declutter";
  import type { Names } from "$lib/core/strings";
  import type { Communities, Core, Territories, Years } from "$lib/data/dataset";
  import { beginStep, endStep, paint } from "$lib/data/progress";
  import {
    buildLabelIndex,
    cameraAffine,
    drawOverlay,
    labelAt,
    pixelsPerUnit,
    prepareTerritories,
    type LabelHit
  } from "$lib/render/overlay";
  import { sizeScaleFor } from "$lib/render/palette";
  import { createTerritoryGl, type TerritoryGl } from "$lib/render/territories.gl";
  import { Graph } from "@cosmos.gl/graph";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";

  import { mark, runBench, type BenchEvent, type BenchMode } from "./bench";
  import { countsWithin, lastDayOfYear, pointColors, visibleSizes } from "./graphdata";
  import type { LinksRequest, LinksResponse, SceneResponse } from "./links.worker";
  import { baseShownFor, detailSlotsFor, type SpaceView } from "./linkselect";
  import { getMapState } from "./state.svelte";

  type Props = {
    core: Core;
    names: Names;
    links: Worker;
    firstYear: Uint16Array;
    years: Years | null;
    territories: Territories;
    communities: Communities;
    labelZooms: LabelZooms | null;
    focus: { node: number; seq: number } | null;
    viewport: { x: number; y: number; zoom: number } | null;
    onviewport?: (v: { x: number; y: number; zoom: number }) => void;
    onlinks?: (drawn: number) => void;
    onready?: () => void;
    onerror?: (message: string) => void;
  };
  let {
    core,
    names,
    links,
    firstYear,
    years,
    territories,
    communities,
    labelZooms,
    focus,
    viewport,
    onviewport,
    onlinks,
    onready,
    onerror
  }: Props = $props();

  const INTRO_CONTRACTION = 0.25;
  const INTRO_MS = 900;
  const LINK_OPACITY = 0.12;
  const pixelRatioCap = 1.5;
  const territoryRatioCap = 1;
  const labelRatioCap = 1.5;
  const labelRatioMoving = 1;

  const ui = getMapState();
  const space = $derived(core.manifest.space_size);
  const yearExtent = $derived<[number, number]>([
    core.manifest.years[0],
    core.manifest.years[core.manifest.years.length - 1]
  ]);
  const pointPositions = $derived(Float32Array.from(core.positions));
  const bounds = $derived.by(() => {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (let i = 0; i < core.positions.length; i += 2) {
      const x = core.positions[i];
      const y = core.positions[i + 1];
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
    return { minX, minY, maxX, maxY };
  });

  let graph = $state.raw<Graph | null>(null);
  let overlay = $state<HTMLCanvasElement>();
  let territoryCanvas = $state<HTMLCanvasElement>();
  let territoryGl: TerritoryGl | null = null;
  $effect(() => {
    if (!territoryCanvas) return;
    const layer = createTerritoryGl(territoryCanvas, territories.regions);
    territoryGl = layer;
    return () => {
      layer?.destroy();
      territoryGl = null;
    };
  });
  let hexLayer = $state<HTMLDivElement>();
  // the size of the --hex-tile svg in app.css; the strip scrolls by whole tiles so it never jumps
  const HEX_PERIOD_X = 30;
  const HEX_PERIOD_Y = 10 * Math.sqrt(3);
  let hexCamera: { zoom: number; x: number; y: number } | null = null;
  let hexX = 0;
  let hexY = 0;
  const slideHex = (g: Graph) => {
    const zoom = g.getZoomLevel();
    const [x, y] = g.spaceToScreenPosition([0, 0]);
    if (hexCamera && hexCamera.zoom === zoom) {
      hexX = (hexX + x - hexCamera.x) % HEX_PERIOD_X;
      hexY = (hexY + y - hexCamera.y) % HEX_PERIOD_Y;
      hexLayer?.style.setProperty("--hex-x", `${hexX}px`);
      hexLayer?.style.setProperty("--hex-y", `${hexY}px`);
    }
    hexCamera = { zoom, x, y };
  };
  let sceneView = $state.raw<SpaceView | null>(null);
  let scene = $state.raw<SceneResponse | null>(null);

  const counts = $derived(
    ui.yearRange === null
      ? core.postCounts
      : countsWithin(core.postCounts, years, core.manifest.years, ui.yearRange)
  );
  const sizes = $derived(visibleSizes(counts, core.categories, ui.categoryMask));
  const colors = $derived(
    pointColors(core.categories, firstYear, { colorMode: ui.colorMode, yearExtent }, sizes)
  );

  let revealed = false;
  let baseShown = 0;
  let detailShown = 0;
  let forcedShown = 0;
  const reportLinks = () => onlinks?.(baseShown + detailShown + forcedShown);
  let density = $state(untrack(() => ui.edgeDensity));
  $effect(() => {
    const target = ui.edgeDensity;
    const timer = setTimeout(() => (density = target), 150);
    return () => clearTimeout(timer);
  });
  const detailSlots = $derived(detailSlotsFor(density));
  const writeDetail = (links: Float32Array, colors: Float32Array, drawn: number) =>
    untrack(() => {
      if (!graph || !scene) return;
      const t0 = performance.now();
      graph.setLinksRange(links, scene.detailStart);
      graph.setLinkColorsRange(colors, scene.detailStart);
      mark("detail.write", performance.now() - t0, { drawn, written: links.length / 2 });
      detailShown = drawn;
      reportLinks();
    });
  $effect(() => {
    links.onmessage = async (event: MessageEvent<LinksResponse>) => {
      const msg = event.data;
      if (msg.type === "error") {
        onerror?.(msg.message);
        return;
      }
      if (msg.type === "base") {
        if (msg.id !== baseId || !graph || !scene) return;
        const t0 = performance.now();
        if (msg.links.length > 0) graph.setLinksRange(msg.links, msg.start);
        baseShown = msg.shown;
        mark("base.write", performance.now() - t0, { shown: msg.shown });
        reportLinks();
        return;
      }
      if (msg.type === "forced") {
        if (msg.id !== forcedId || !graph || !scene) return;
        const t0 = performance.now();
        const start = scene.forcedStart;
        if (msg.links.length > 0) graph.setLinksRange(msg.links, start);
        if (msg.drawn > 0) graph.setLinkColorsRange(msg.colors, start);
        forcedShown = msg.drawn;
        graph.setConfigPartial(
          msg.node === null
            ? {
                highlightedPointIndices: undefined,
                highlightedLinkIndices: undefined,
                focusedPointIndex: undefined
              }
            : {
                highlightedPointIndices: Array.from(msg.neighbors),
                highlightedLinkIndices: Array.from({ length: msg.drawn }, (_, l) => start + l),
                focusedPointIndex: msg.node
              }
        );
        mark("forced.tail", performance.now() - t0, {
          drawn: msg.drawn,
          neighbors: msg.neighbors.length
        });
        reportLinks();
        return;
      }
      if (msg.type === "scene") {
        if (msg.id !== sceneId) return;
        mark("worker.scene", msg.selectMs, { links: msg.linkCount });
        if (!revealed) {
          endStep("choosing edges");
          beginStep("drawing edges");
        }
        await paint();
        if (msg.id !== sceneId) return;
        scene = msg;
        return;
      }
      if (msg.id !== detailId) return;
      mark("worker.detail", msg.selectMs, {
        links: msg.drawn,
        ink: Math.round(msg.ink * 10) / 10,
        clipped: msg.clipped ? 1 : 0,
        fetchMs: Math.round(msg.fetchMs),
        levels: msg.levels
      });
      if (msg.clipped && dev)
        console.warn(
          `detail run clipped at ${msg.drawn} links, ${msg.ink.toFixed(1)}x of ${density}x`
        );
      writeDetail(msg.links, msg.colors, msg.drawn);
    };
    return () => {
      links.onmessage = null;
    };
  });
  $effect(() => {
    if (!revealed) beginStep("choosing edges");
    const request: LinksRequest = {
      type: "scene",
      id: ++sceneId,
      query: {
        showImplications: ui.showImplications,
        detailSlots,
        asOfDay:
          ui.showImplications && ui.yearRange !== null ? lastDayOfYear(ui.yearRange[1]) : null
      }
    };
    links.postMessage(request);
  });
  $effect(() => {
    if (!scene) return;
    const ppu = graph ? pixelsPerUnit(graph) : 0;
    const w = overlay?.clientWidth ?? 0;
    const h = overlay?.clientHeight ?? 0;
    const request: LinksRequest = {
      type: "detail",
      id: ++detailId,
      view: {
        view: sceneView,
        ppu,
        screenArea: Math.max(1, w * h),
        inkTarget: density,
        slots: scene.detailSlots,
        previousDrawn: untrack(() => detailShown)
      }
    };
    links.postMessage(request);
  });
  $effect(() => {
    if (!scene) return;
    const request: LinksRequest = {
      type: "base",
      id: ++baseId,
      shown: baseShownFor(density, scene.linkCount),
      previousShown: untrack(() => baseShown)
    };
    links.postMessage(request);
  });
  $effect(() => {
    if (!scene) return;
    const request: LinksRequest = {
      type: "forced",
      id: ++forcedId,
      node: ui.selected,
      previousDrawn: untrack(() => forcedShown)
    };
    links.postMessage(request);
  });

  const labelIndex = $derived(buildLabelIndex(core.positions, space));
  const overlayScene = $derived({
    positions: pointPositions,
    index: labelIndex,
    names,
    postCounts: core.postCounts,
    categories: core.categories,
    sizes,
    regions: prepareTerritories(
      territories.regions,
      communities.regions.map((c) => c.name)
    ),
    communities,
    territories: ui.territories,
    labels: ui.labels,
    zooms: labelZooms,
    selected: ui.selected,
    hovered: ui.hovered,
    path: ui.path?.nodes ?? null
  });
  let latestOverlay: typeof overlayScene | null = null;
  let labelHits: LabelHit[] = [];
  let frame = 0;
  let cameraMoving = false;
  let introSpread = 1;
  const scheduleOverlay = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (!graph || !overlay || !latestOverlay) return;
      const dpr = window.devicePixelRatio || 1;
      const labelRatio = Math.min(dpr, cameraMoving ? labelRatioMoving : labelRatioCap);
      const t0 = performance.now();
      labelHits = drawOverlay(graph, overlay, latestOverlay, labelRatio, introSpread);
      const t1 = performance.now();
      if (territoryGl) {
        if (latestOverlay.territories)
          territoryGl.draw(
            cameraAffine(graph),
            overlay.clientWidth,
            overlay.clientHeight,
            Math.min(dpr, territoryRatioCap)
          );
        else territoryGl.clear();
      }
      mark("overlay.labels", t1 - t0, { labels: labelHits.length, ratio: labelRatio });
      mark("overlay.territory", performance.now() - t1, {});
    });
  };
  $effect(() => {
    latestOverlay = overlayScene;
    scheduleOverlay();
  });

  const reportViewport = () => {
    if (!graph || !overlay || !onviewport) return;
    const [x, y] = graph.screenToSpacePosition([overlay.clientWidth / 2, overlay.clientHeight / 2]);
    onviewport({ x, y, zoom: graph.getZoomLevel() });
  };

  let settleTimer = 0;
  const settleZoom = () => {
    clearTimeout(settleTimer);
    settleTimer = window.setTimeout(() => {
      cameraMoving = false;
      scheduleOverlay();
      if (!graph || !overlay) return;
      const [x0, y0] = graph.screenToSpacePosition([0, 0]);
      const [x1, y1] = graph.screenToSpacePosition([overlay.clientWidth, overlay.clientHeight]);
      const view: SpaceView = {
        minX: Math.min(x0, x1),
        minY: Math.min(y0, y1),
        maxX: Math.max(x0, x1),
        maxY: Math.max(y0, y1)
      };
      const whole =
        view.minX <= bounds.minX &&
        view.maxX >= bounds.maxX &&
        view.minY <= bounds.minY &&
        view.maxY >= bounds.maxY;
      const inside =
        sceneView !== null &&
        view.minX >= sceneView.minX &&
        view.maxX <= sceneView.maxX &&
        view.minY >= sceneView.minY &&
        view.maxY <= sceneView.maxY;
      const area = (view.maxX - view.minX) * (view.maxY - view.minY);
      const lastArea = sceneView
        ? (sceneView.maxX - sceneView.minX) * (sceneView.maxY - sceneView.minY)
        : Infinity;
      const zoomedInALot = area < lastArea / 9;
      if (whole) {
        if (sceneView !== null) sceneView = null;
      } else if (!inside || zoomedInALot) {
        const mx = view.maxX - view.minX;
        const my = view.maxY - view.minY;
        sceneView = {
          minX: view.minX - mx,
          minY: view.minY - my,
          maxX: view.maxX + mx,
          maxY: view.maxY + my
        };
      }
      reportViewport();
    }, 200);
  };

  const setup: Attachment<HTMLDivElement> = (div) => {
    let sizeScale = 1;
    let zoomTuned = false;
    const g = new Graph(div, {
      backgroundColor: "rgba(0, 0, 0, 0)",
      spaceSize: space,
      enableSimulation: false,
      enableDrag: false,
      enableZoom: true,
      renderLinks: true,
      linkVisibilityDistanceRange: [30, 500],
      linkVisibilityMinTransparency: 0.15,
      linkOpacity: LINK_OPACITY,
      linkGreyoutOpacity: 0.02,
      pointGreyoutOpacity: 0.12,
      renderHoveredPointRing: false,
      focusedPointRingColor: cssToken("--color-accent"),
      fitViewOnInit: false,
      transitionDuration: 0,
      transitionEasing: "exp-out",
      onTransition: (progress) => {
        cameraMoving = true;
        const k = INTRO_CONTRACTION + (1 - INTRO_CONTRACTION) * progress;
        const c = space / 2;
        const from = core.positions;
        const to = pointPositions;
        for (let i = 0; i < from.length; i++) to[i] = c + (from[i] - c) * k;
        introSpread = k;
        scheduleOverlay();
      },
      onTransitionEnd: () => {
        pointPositions.set(core.positions);
        introSpread = 1;
        cameraMoving = false;
        scheduleOverlay();
      },
      scalePointsOnZoom: false,
      pixelRatio: Math.min(window.devicePixelRatio || 1, pixelRatioCap),
      showFPSMonitor: dev,
      onPointClick: (index, _position, event) => {
        ui.selected = sizes[index] > 0 ? index : labelAt(labelHits, event.offsetX, event.offsetY);
      },
      onBackgroundClick: (event) => {
        ui.selected = labelAt(labelHits, event.offsetX, event.offsetY);
      },
      onPointMouseOver: (index) => {
        ui.hovered = sizes[index] > 0 ? index : null;
        div.style.cursor = ui.hovered === null ? "" : "pointer";
      },
      onPointMouseOut: () => {
        ui.hovered = null;
        div.style.cursor = "";
      },
      onMouseMove: (index, _position, event) => {
        if (index !== undefined && sizes[index] > 0) return;
        const hit = labelAt(labelHits, event.offsetX, event.offsetY);
        if (hit !== ui.hovered) ui.hovered = hit;
        div.style.cursor = hit === null ? "" : "pointer";
      },
      onZoomStart: (e) => {
        if (zoomTuned) return;
        zoomTuned = true;
        e.target
          .scaleExtent([ZOOM_MIN, ZOOM_MAX])
          // deltaMode 0 is pixels, 1 lines, 2 pages; convert to pixels before scaling
          .wheelDelta(
            (w) =>
              (-w.deltaY *
                (w.deltaMode === 1 ? 25 : w.deltaMode ? 500 : 1) *
                ZOOM_PER_NOTCH *
                (w.ctrlKey ? 10 : 1)) /
              100
          )
          .filter((w) => w.type !== "dblclick" && (!w.ctrlKey || w.type === "wheel") && !w.button);
      },
      onZoom: () => {
        cameraMoving = true;
        slideHex(g);
        const s = sizeScaleFor(pixelsPerUnit(g));
        if (Math.abs(s - sizeScale) > 0.02) {
          sizeScale = s;
          g.setConfigPartial({ pointSizeScale: s });
        }
        scheduleOverlay();
      },
      onZoomEnd: () => {
        scheduleOverlay();
        settleZoom();
      }
    });
    const target = viewport ?? {
      x: space / 2,
      y: space / 2,
      zoom: Math.min(div.clientWidth, div.clientHeight) / space
    };
    g.setZoomTransformByPointPositions(
      new Float32Array([target.x, target.y]),
      0,
      target.zoom,
      undefined,
      false
    );
    graph = g;
    if (dev) Object.assign(window, { atlas621: g });

    return () => {
      g.destroy();
      graph = null;
    };
  };

  $effect(() => {
    if (!graph) return;
    const start = Float32Array.from(core.positions);
    const c = space / 2;
    for (let i = 0; i < start.length; i++) start[i] = c + (start[i] - c) * INTRO_CONTRACTION;
    graph.setPointPositions(start, true);
    graph.render();
    if (!revealed) endStep("placing tags");
  });

  $effect(() => {
    if (!graph) return;
    const t0 = performance.now();
    graph.setPointSizes(sizes);
    graph.render();
    mark("points.sizes", performance.now() - t0, { points: sizes.length });
    scheduleOverlay();
  });

  $effect(() => {
    if (!graph) return;
    const t0 = performance.now();
    graph.setPointColors(colors);
    graph.render();
    mark("points.colors", performance.now() - t0, { points: colors.length / 4 });
  });

  $effect(() => {
    if (!graph || !scene) return;
    const t0 = performance.now();
    graph.setLinks(scene.links);
    graph.setLinkStyles(scene.styles ?? undefined);
    graph.setLinkArrows(scene.arrows ? Array.from(scene.arrows, (v) => v === 1) : undefined);
    graph.setLinkColors(scene.linkColors);
    graph.render();
    mark("links.upload", performance.now() - t0, { links: scene.links.length / 2 });
    if (!revealed) endStep("drawing edges");
    baseShown = scene.linkCount;
    detailShown = 0;
    forcedShown = 0;
    scheduleOverlay();
  });

  $effect(() => {
    if (!graph || !scene || revealed) return;
    revealed = true;
    const g = graph;
    beginStep("compiling shaders");
    // one frame so the step label paints, one to draw the first labels, one to clear the
    // step before the intro animation starts
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (overlay && latestOverlay)
          labelHits = drawOverlay(
            g,
            overlay,
            latestOverlay,
            Math.min(window.devicePixelRatio || 1, labelRatioCap)
          );
        requestAnimationFrame(() => {
          endStep("compiling shaders");
          g.setPointPositions(core.positions, true);
          g.render(undefined, INTRO_MS);
          onready?.();
        });
      });
    });
  });

  export const bench = (
    mode: BenchMode,
    report: (event: BenchEvent) => void,
    signal: AbortSignal
  ) => {
    if (!graph || !overlay) return;
    return runBench(
      {
        graph,
        overlay,
        ui,
        positions: core.positions,
        space,
        names,
        years: core.manifest.years,
        nodes: core.manifest.nodes,
        edges: core.manifest.edges,
        settings: { pixelRatio: pixelRatioCap }
      },
      mode,
      report,
      signal
    );
  };

  $effect(() => {
    if (!graph || !focus) return;
    const x = core.positions[focus.node * 2];
    const y = core.positions[focus.node * 2 + 1];
    graph.setZoomTransformByPointPositions(
      new Float32Array([x, y]),
      700,
      Math.max(graph.getZoomLevel(), 5),
      undefined,
      false
    );
  });
</script>

<svelte:window onresize={scheduleOverlay} />

<div class="absolute inset-0">
  <div bind:this={hexLayer} class="hex-strip pointer-events-none absolute inset-0"></div>
  <div class="absolute inset-0" {@attach setup}></div>
  <canvas bind:this={territoryCanvas} class="pointer-events-none absolute inset-0 h-full w-full"
  ></canvas>
  <canvas bind:this={overlay} class="pointer-events-none absolute inset-0 h-full w-full"></canvas>
</div>
