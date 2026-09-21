import type { Communities, Territory, TerritoryLevel as TerritoryData } from "$lib/data/dataset";
import type { Names } from "$lib/data/names";
import type { Graph } from "@cosmos.gl/graph";

import {
  LABEL_HALF_MAX,
  LABEL_LEFT,
  LABEL_PAD,
  labelFontSize,
  zoomBucket,
  type LabelZooms
} from "./labels";
import { CATEGORY_COLORS, communityColor, cssToken, sizeScaleFor } from "./palette";
import { hexCenter } from "./territories.gl";

export type TerritoryLevel = {
  features: (Territory & {
    bbox: [number, number, number, number];
  })[];
  names: string[];
};

const SQRT3 = Math.sqrt(3);

export const prepareTerritories = (level: TerritoryData, names: string[]): TerritoryLevel => {
  const radius = level.hex_size;
  const half = (radius * SQRT3) / 2;
  return {
    names,
    features: level.features.map((f) => {
      let minX = Infinity;
      let minY = Infinity;
      let maxX = -Infinity;
      let maxY = -Infinity;
      for (let i = 0; i < f.hexes.length; i += 2) {
        const q = f.hexes[i];
        const r = f.hexes[i + 1];
        const [cx, cy] = hexCenter(level, q, r);
        if (cx - radius < minX) minX = cx - radius;
        if (cx + radius > maxX) maxX = cx + radius;
        if (cy - half < minY) minY = cy - half;
        if (cy + half > maxY) maxY = cy + half;
      }
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

type Sprite = { x: number; y: number; w: number; h: number };
type Draw = { sp: Sprite; x: number; y: number; w: number; h: number };
type Atlas = {
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  dpr: number;
  sprites: Map<string, Sprite>;
  shelfY: number;
  shelfH: number;
  cursorX: number;
};
type Box = { width: number; height: number };
const ATLAS_SIZE = 2048;
const atlases = new Map<number, Atlas>();
const boxCache = new Map<string, Box>();
let probe: CanvasRenderingContext2D | null = null;

const measure = (key: string, text: string, font: string): Box => {
  let box = boxCache.get(key);
  if (box) return box;
  if (boxCache.size > 20000) boxCache.clear();
  probe ??= document.createElement("canvas").getContext("2d");
  if (!probe) throw new Error("overlay: no 2d context");
  probe.font = font;
  const size = Number.parseInt(font.match(/(\d+)px/)?.[1] ?? "12", 10);
  box = {
    width: Math.ceil(probe.measureText(text).width) + LABEL_PAD * 2,
    height: Math.ceil(size * 1.4) + LABEL_PAD * 2
  };
  boxCache.set(key, box);
  return box;
};

const atlasFor = (dpr: number): Atlas => {
  let atlas = atlases.get(dpr);
  if (atlas) return atlas;
  const canvas = document.createElement("canvas");
  canvas.width = ATLAS_SIZE;
  canvas.height = ATLAS_SIZE;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("overlay: no 2d context");
  atlas = { canvas, ctx, dpr, sprites: new Map(), shelfY: 0, shelfH: 0, cursorX: 0 };
  atlases.set(dpr, atlas);
  return atlas;
};

const sprite = (atlas: Atlas, key: string, text: string, font: string, fill: string): Sprite => {
  let sp = atlas.sprites.get(key);
  if (sp) return sp;
  const { width, height } = measure(key, text, font);
  const w = Math.ceil(width * atlas.dpr);
  const h = Math.ceil(height * atlas.dpr);
  if (atlas.cursorX + w > ATLAS_SIZE) {
    atlas.shelfY += atlas.shelfH;
    atlas.shelfH = 0;
    atlas.cursorX = 0;
  }
  if (atlas.shelfY + Math.max(h, atlas.shelfH) > ATLAS_SIZE) {
    atlas.ctx.clearRect(0, 0, ATLAS_SIZE, ATLAS_SIZE);
    atlas.sprites.clear();
    atlas.shelfY = 0;
    atlas.shelfH = 0;
    atlas.cursorX = 0;
  }
  sp = { x: atlas.cursorX, y: atlas.shelfY, w, h };
  atlas.cursorX += w;
  if (h > atlas.shelfH) atlas.shelfH = h;
  const ctx = atlas.ctx;
  ctx.save();
  ctx.translate(sp.x, sp.y);
  ctx.scale(atlas.dpr, atlas.dpr);
  ctx.font = font;
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.lineWidth = 3;
  ctx.strokeStyle = cssToken("--color-page-dim");
  ctx.globalAlpha = 0.9;
  ctx.strokeText(text, LABEL_PAD, height / 2);
  ctx.globalAlpha = 1;
  ctx.fillStyle = fill;
  ctx.fillText(text, LABEL_PAD, height / 2);
  ctx.restore();
  atlas.sprites.set(key, sp);
  return sp;
};

const NAME_SPRITE_PX = 24;

const placeTerritoryNames = (
  terr: TerritoryLevel,
  screen: Rect,
  sx: number,
  sy: number,
  tx: number,
  ty: number,
  atlas: Atlas,
  blockers: Rect[],
  draws: Draw[]
) => {
  const ppu = Math.abs(sx);
  const placed: Rect[] = [];
  for (const f of terr.features) {
    if (!f.label) continue;
    const widthPx = (f.bbox[2] - f.bbox[0]) * ppu;
    if (widthPx < 70) continue;
    const name = terr.names[f.community] ?? "";
    const size = Math.max(11, Math.min(NAME_SPRITE_PX, 7 + Math.sqrt(widthPx) * 0.8));
    const [r, g, b] = communityColor(f.community);
    const fill = `rgba(${Math.min(255, r + 60)},${Math.min(255, g + 60)},${Math.min(255, b + 60)},0.95)`;
    const key = `t|${f.community}`;
    const font = `600 ${NAME_SPRITE_PX}px ${cssToken("--font-sans")}`;
    const box = measure(key, name, font);
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
    draws.push({ sp: sprite(atlas, key, name, font, fill), x: rect[0], y: rect[1], w, h });
  }
};

export const pixelsPerUnit = (graph: Graph): number => Math.abs(cameraAffine(graph).sx);

export type LabelHit = { node: number; rect: Rect };

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
  const draws: Draw[] = [];
  const blit = () => {
    for (const d of draws)
      ctx.drawImage(atlas.canvas, d.sp.x, d.sp.y, d.sp.w, d.sp.h, d.x, d.y, d.w, d.h);
    return hits;
  };

  const terr = s.territories ? s.regions : null;
  if (terr) placeTerritoryNames(terr, screen, sx, sy, tx, ty, atlas, blockers, draws);

  const sizeScale = sizeScaleFor(ppu);
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

  if (!s.labels || !s.names || !s.zooms) return blit();
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
    const box = measure(key, text, font);
    const lx = x + (s.sizes[i] * sizeScale) / 2 + LABEL_LEFT;
    const rect: Rect = [lx, y - box.height / 2, lx + box.width, y + box.height / 2];
    if (!strong && (!overlaps(rect, screen) || blockers.some((p) => overlaps(p, rect))))
      return null;
    hits.push({ node: i, rect });
    draws.push({
      sp: sprite(atlas, key, text, font, fill),
      x: rect[0],
      y: rect[1],
      w: box.width,
      h: box.height
    });
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
  if (prefix <= covered) {
    for (let k = 0; k < prefix; k++) place(order[k]);
  } else {
    for (let cy = cy0; cy <= cy1; cy++)
      for (let cx = cx0; cx <= cx1; cx++) {
        const c = cx + cy * index.cells;
        for (let e = index.off[c]; e < index.off[c + 1]; e++) place(index.nodes[e]);
      }
  }
  return blit();
};

export const labelAt = (hits: LabelHit[], x: number, y: number): number | null => {
  for (const h of hits)
    if (x >= h.rect[0] && x <= h.rect[2] && y >= h.rect[1] && y <= h.rect[3]) return h.node;
  return null;
};
