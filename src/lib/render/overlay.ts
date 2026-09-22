import { atlasFor, measure, sprite } from "$lib/core/atlas";
import { cssToken } from "$lib/core/css";
import { zoomBucket, type LabelZooms } from "$lib/core/declutter";
import { eachHexCell, hexCenter, hexHalfHeight } from "$lib/core/hex";
import type { Names } from "$lib/core/strings";
import type { Communities, Territory, TerritoryLevel as TerritoryData } from "$lib/data/dataset";
import type { Graph } from "@cosmos.gl/graph";

import { LABEL_HALF_MAX, LABEL_LEFT, LABEL_PAD, labelFontSize } from "./labels";
import { CATEGORY_COLORS, communityColor, sizeScaleFor } from "./palette";

export type TerritoryLevel = {
  features: (Territory & {
    bbox: [number, number, number, number];
  })[];
  names: string[];
};

export const prepareTerritories = (level: TerritoryData, names: string[]): TerritoryLevel => {
  const grid = { size: level.hex_size, origin: level.origin };
  const radius = level.hex_size;
  const half = hexHalfHeight(grid);
  return {
    names,
    features: level.features.map((f) => {
      let minX = Infinity;
      let minY = Infinity;
      let maxX = -Infinity;
      let maxY = -Infinity;
      eachHexCell(f.hexes, (q, r) => {
        const [cx, cy] = hexCenter(grid, q, r);
        if (cx - radius < minX) minX = cx - radius;
        if (cx + radius > maxX) maxX = cx + radius;
        if (cy - half < minY) minY = cy - half;
        if (cy + half > maxY) maxY = cy + half;
      });
      return { ...f, bbox: [minX, minY, maxX, maxY] };
    })
  };
};

export type LabelIndex = { cells: number; size: number; off: Uint32Array; nodes: Uint32Array };

export const buildLabelIndex = (positions: Float32Array, space: number, cells = 64): LabelIndex => {
  const n = positions.length / 2;
  const size = space / cells;
  const cellOf = (i: number) => {
    const cx = Math.min(cells - 1, Math.max(0, Math.floor(positions[i * 2] / size)));
    const cy = Math.min(cells - 1, Math.max(0, Math.floor(positions[i * 2 + 1] / size)));
    return cx + cy * cells;
  };
  const off = new Uint32Array(cells * cells + 1);
  for (let i = 0; i < n; i++) off[cellOf(i) + 1]++;
  for (let c = 0; c < cells * cells; c++) off[c + 1] += off[c];
  const fill = off.slice(0, cells * cells);
  const nodes = new Uint32Array(n);
  for (let i = 0; i < n; i++) nodes[fill[cellOf(i)]++] = i;
  return { cells, size, off, nodes };
};

export type OverlayScene = {
  positions: Float32Array;
  index: LabelIndex;
  names: Names | null;
  postCounts: Uint32Array;
  categories: Uint8Array;
  sizes: Float32Array;
  regions: TerritoryLevel | null;
  communities: Communities | null;
  territories: boolean;
  labels: boolean;
  zooms: LabelZooms | null;
  selected: number | null;
  hovered: number | null;
  path: number[] | null;
};

type Rect = [number, number, number, number];

const overlaps = (a: Rect, b: Rect) => a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];

export const cameraAffine = (graph: Graph) => {
  const [tx, ty] = graph.spaceToScreenPosition([0, 0]);
  const [x1] = graph.spaceToScreenPosition([1, 0]);
  const [, y1] = graph.spaceToScreenPosition([0, 1]);
  return { sx: x1 - tx, sy: y1 - ty, tx, ty };
};

// one sprite blit, held back until every sprite for the frame has been rasterised
type Blit = {
  key: string;
  text: string;
  font: string;
  fill: string;
  x: number;
  y: number;
  w: number;
  h: number;
};

const NAME_SPRITE_PX = 24;

const placeTerritoryNames = (
  terr: TerritoryLevel,
  screen: Rect,
  sx: number,
  sy: number,
  tx: number,
  ty: number,
  blockers: Rect[],
  queue: Blit[]
) => {
  const ppu = Math.abs(sx);
  const placed: Rect[] = [];
  for (const f of terr.features) {
    const widthPx = (f.bbox[2] - f.bbox[0]) * ppu;
    if (widthPx < 70) continue;
    const name = terr.names[f.community] ?? "";
    const size = Math.max(11, Math.min(NAME_SPRITE_PX, 7 + Math.sqrt(widthPx) * 0.8));
    const [r, g, b] = communityColor(f.community);
    const fill = `rgba(${Math.min(255, r + 60)},${Math.min(255, g + 60)},${Math.min(255, b + 60)},0.95)`;
    const key = `t|${f.community}`;
    const font = `600 ${NAME_SPRITE_PX}px ${cssToken("--font-sans")}`;
    const box = measure(key, name, font, LABEL_PAD);
    const scale = size / NAME_SPRITE_PX;
    const w = box.width * scale;
    const h = box.height * scale;
    const x = f.anchor[0] * sx + tx;
    const y = f.anchor[1] * sy + ty;
    const rect: Rect = [x - w / 2, y - h / 2, x + w / 2, y + h / 2];
    if (placed.some((p) => overlaps(p, rect))) continue;
    placed.push(rect);
    if (!overlaps(rect, screen)) continue;
    blockers.push(rect);
    queue.push({ key, text: name, font, fill, x: rect[0], y: rect[1], w, h });
  }
};

export const pixelsPerUnit = (graph: Graph): number => Math.abs(cameraAffine(graph).sx);

export type LabelHit = { node: number; rect: Rect };

// during the intro s.positions are pulled toward the centre by spread, while s.index was built
// from the uncontracted layout; 1 once the animation has finished
export const drawOverlay = (
  graph: Graph,
  canvas: HTMLCanvasElement,
  s: OverlayScene,
  dpr: number,
  spread = 1
): LabelHit[] => {
  const hits: LabelHit[] = [];
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return hits;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const { sx, sy, tx, ty } = cameraAffine(graph);
  const ppu = Math.abs(sx);
  const screen: Rect = [0, 0, w, h];
  const blockers: Rect[] = [];
  const atlas = atlasFor(dpr);
  const outline = cssToken("--color-page-dim");
  const queue: Blit[] = [];
  // every sprite is rasterised into the atlas before any is read back out of it. interleaving
  // the two costs a surface sync per label where the driver cannot share the atlas canvas
  const flush = (): LabelHit[] => {
    const rasterise = () =>
      queue.map((b) => sprite(atlas, b.key, b.text, b.font, b.fill, outline, LABEL_PAD));
    const before = atlas.generation;
    // a recycle part way through invalidates the sprites handed out before it; the frame's
    // labels fit in an empty atlas, so rasterising once more is enough
    const sprites = rasterise();
    const fresh = atlas.generation === before ? sprites : rasterise();
    for (let i = 0; i < queue.length; i++) {
      const b = queue[i];
      const sp = fresh[i];
      ctx.drawImage(atlas.canvas, sp.x, sp.y, sp.w, sp.h, b.x, b.y, b.w, b.h);
    }
    return hits;
  };
  const sizeScale = sizeScaleFor(ppu);

  // the ring and the path go under every label
  if (s.hovered !== null && s.sizes[s.hovered] > 0) {
    const x = s.positions[s.hovered * 2] * sx + tx;
    const y = s.positions[s.hovered * 2 + 1] * sy + ty;
    ctx.beginPath();
    ctx.arc(x, y, (s.sizes[s.hovered] * sizeScale) / 2 + 3, 0, Math.PI * 2);
    ctx.lineWidth = 1.5;
    ctx.strokeStyle = "rgba(255,255,255,0.9)";
    ctx.stroke();
  }

  if (s.path && s.path.length > 1) {
    ctx.beginPath();
    s.path.forEach((i, k) => {
      const x = s.positions[i * 2] * sx + tx;
      const y = s.positions[i * 2 + 1] * sy + ty;
      if (k === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.lineWidth = 6;
    ctx.strokeStyle = "rgba(0,0,0,0.6)";
    ctx.stroke();
    ctx.lineWidth = 2.5;
    ctx.strokeStyle = "rgba(255,255,255,0.9)";
    ctx.stroke();
    ctx.fillStyle = "#fff";
    for (const i of s.path) {
      ctx.beginPath();
      ctx.arc(s.positions[i * 2] * sx + tx, s.positions[i * 2 + 1] * sy + ty, 3.5, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  const terr = s.territories ? s.regions : null;
  if (terr) placeTerritoryNames(terr, screen, sx, sy, tx, ty, blockers, queue);

  if (!s.labels || !s.names || !s.zooms) return flush();
  const names = s.names;
  const label = (i: number, strong: boolean): Rect | null => {
    const x = s.positions[i * 2] * sx + tx;
    const y = s.positions[i * 2 + 1] * sy + ty;
    const size = strong ? 14 : labelFontSize(s.postCounts[i]);
    const [r, g, b] = CATEGORY_COLORS[s.categories[i]] ?? CATEGORY_COLORS[0];
    const fill = strong ? "#fff" : `rgba(${r},${g},${b},0.95)`;
    const text = names.at(i);
    const key = `${strong ? "s" : "p"}${size}|${s.categories[i]}|${text}`;
    const font = `${strong ? 700 : 500} ${size}px ${cssToken("--font-sans")}`;
    const box = measure(key, text, font, LABEL_PAD);
    const lx = x + (s.sizes[i] * sizeScale) / 2 + LABEL_LEFT;
    const rect: Rect = [lx, y - box.height / 2, lx + box.width, y + box.height / 2];
    if (!strong && (!overlaps(rect, screen) || blockers.some((p) => overlaps(p, rect))))
      return null;
    hits.push({ node: i, rect });
    queue.push({ key, text, font, fill, x: rect[0], y: rect[1], w: box.width, h: box.height });
    return rect;
  };
  const forced = (i: number | null) => {
    if (i === null || s.sizes[i] <= 0) return;
    const rect = label(i, true);
    if (rect) blockers.push(rect);
  };
  forced(s.selected);
  if (s.hovered !== s.selected) forced(s.hovered);
  for (const i of s.path ?? []) if (i !== s.selected && i !== s.hovered) forced(i);

  const { zoom, order, starts, reach } = s.zooms;
  const ppuEff = ppu * spread;
  const worldX = (px: number) => (px - tx) / sx;
  const worldY = (py: number) => (py - ty) / sy;
  const minX = Math.min(worldX(-reach), worldX(w));
  const maxX = Math.max(worldX(-reach), worldX(w));
  const minY = Math.min(worldY(-LABEL_HALF_MAX), worldY(h + LABEL_HALF_MAX));
  const maxY = Math.max(worldY(-LABEL_HALF_MAX), worldY(h + LABEL_HALF_MAX));
  const place = (i: number) => {
    if (zoom[i] > ppuEff || s.sizes[i] <= 0 || i === s.selected || i === s.hovered) return;
    if (s.path?.includes(i)) return;
    const x = s.positions[i * 2];
    const y = s.positions[i * 2 + 1];
    if (x < minX || x > maxX || y < minY || y > maxY) return;
    label(i, false);
  };
  const prefix = starts[zoomBucket(ppuEff) + 1];
  const { index } = s;
  const centre = (index.cells * index.size) / 2;
  // undo the contraction to look the coordinate up in the uncontracted index
  const clampCell = (v: number) =>
    Math.min(
      index.cells - 1,
      Math.max(0, Math.floor((centre + (v - centre) / spread) / index.size))
    );
  const cx0 = clampCell(minX);
  const cx1 = clampCell(maxX);
  const cy0 = clampCell(minY);
  const cy1 = clampCell(maxY);
  let covered = 0;
  for (let cy = cy0; cy <= cy1; cy++)
    for (let cx = cx0; cx <= cx1; cx++) {
      const c = cx + cy * index.cells;
      covered += index.off[c + 1] - index.off[c];
    }
  // both reach the same labels; walk whichever list is shorter at this zoom
  if (prefix <= covered) {
    for (let k = 0; k < prefix; k++) place(order[k]);
  } else {
    for (let cy = cy0; cy <= cy1; cy++)
      for (let cx = cx0; cx <= cx1; cx++) {
        const c = cx + cy * index.cells;
        for (let e = index.off[c]; e < index.off[c + 1]; e++) place(index.nodes[e]);
      }
  }
  return flush();
};

export const labelAt = (hits: LabelHit[], x: number, y: number): number | null => {
  for (const h of hits)
    if (x >= h.rect[0] && x <= h.rect[2] && y >= h.rect[1] && y <= h.rect[3]) return h.node;
  return null;
};
