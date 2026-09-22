import type { Implication } from "$lib/data/dataset";
import { WEIGHT_MAX } from "$lib/data/manifest";
import type { AdjacencyShard, EdgeRun } from "$lib/data/tiles";

export type SpaceView = { minX: number; minY: number; maxX: number; maxY: number };

export const FULL_DENSITY = 14;
export const MAX_DENSITY = 40;
export const detailSlotsFor = (density: number): number =>
  Math.ceil((density / FULL_DENSITY) * 2) * 200_000;
export const baseShownFor = (density: number, count: number): number =>
  Math.round(count * Math.min(1, density / FULL_DENSITY));

export type LinkInput = {
  positions: Float32Array;
  postCounts: Uint32Array;
  base: EdgeRun;
  implications: Implication[];
  edgeCount: number;
  maxDegree: number;
  space: number;
  tileCutoffs: number[];
  tiles: string[];
  adjShardSize: number;
  version: string;
};

export type LinkQuery = {
  showImplications: boolean;
  asOfDay: number | null;
  detailSlots: number;
};

export type LinkScene = {
  links: Float32Array;
  styles: Float32Array | null;
  arrows: Uint8Array | null;
  linkColors: Float32Array;
  linkCount: number;
  detailStart: number;
  detailSlots: number;
  forcedStart: number;
};

export const linkColorAt = (out: Float32Array, l: number, weight: number): void => {
  out[l * 4] = 0.85;
  out[l * 4 + 1] = 0.9;
  out[l * 4 + 2] = 1;
  out[l * 4 + 3] = 0.25 + 0.75 * Math.sqrt(weight / WEIGHT_MAX);
};

// unused link slots point a node at itself, and a zero-length line draws nothing
export const parkRun = (links: Float32Array, start: number, slots: number): void => {
  for (let k = 0; k < slots; k++) {
    links[(start + k) * 2] = k;
    links[(start + k) * 2 + 1] = k;
  }
};

export const baseMembership = (base: EdgeRun, edgeCount: number): Uint8Array => {
  const inBase = new Uint8Array(edgeCount);
  for (const e of base.index) inBase[e] = 1;
  return inBase;
};

export const buildScene = (d: LinkInput, q: LinkQuery): LinkScene => {
  const shown = (i: number) => d.postCounts[i] > 0;
  const { base } = d;
  const count = base.index.length;

  const impl: Implication[] = [];
  if (q.showImplications) {
    for (const item of d.implications) {
      const [child, parent, day] = item;
      const born = q.asOfDay === null || day === null || day <= q.asOfDay;
      if (born && shown(child) && shown(parent)) impl.push(item);
    }
  }

  const detailStart = count;
  const forcedStart = detailStart + q.detailSlots;
  const implStart = forcedStart + d.maxDegree;
  const total = implStart + impl.length;
  const links = new Float32Array(total * 2);
  const linkColors = new Float32Array(total * 4);
  for (let l = 0; l < count; l++) {
    links[l * 2] = base.a[l];
    links[l * 2 + 1] = base.b[l];
    linkColorAt(linkColors, l, base.weight[l]);
  }
  parkRun(links, detailStart, q.detailSlots);
  parkRun(links, forcedStart, d.maxDegree);
  let styles: Float32Array | null = null;
  let arrows: Uint8Array | null = null;
  if (impl.length > 0) {
    styles = new Float32Array(total);
    arrows = new Uint8Array(total);
    for (let j = 0; j < impl.length; j++) {
      const [child, parent, , approx] = impl[j];
      const l = implStart + j;
      links[l * 2] = child;
      links[l * 2 + 1] = parent;
      styles[l] = approx ? 2 : 1;
      arrows[l] = 1;
      linkColors[l * 4] = 0.5;
      linkColors[l * 4 + 1] = 0.85;
      linkColors[l * 4 + 2] = 1;
      linkColors[l * 4 + 3] = 0.9;
    }
  }
  return {
    links,
    styles,
    arrows,
    linkColors,
    linkCount: count,
    detailStart,
    detailSlots: q.detailSlots,
    forcedStart
  };
};

export type LinkBase = {
  links: Float32Array;
  start: number;
  shown: number;
};

export const selectBase = (base: EdgeRun, shown: number, previousShown: number): LinkBase => {
  const start = Math.min(shown, previousShown);
  const end = Math.max(shown, previousShown);
  const links = new Float32Array((end - start) * 2);
  for (let l = start; l < shown; l++) {
    links[(l - start) * 2] = base.a[l];
    links[(l - start) * 2 + 1] = base.b[l];
  }
  parkRun(links, shown - start, end - shown);
  return { links, start, shown };
};

export type LinkDetail = {
  links: Float32Array;
  colors: Float32Array;
  drawn: number;
  clipped: boolean;
  ink: number;
};

export type DetailView = {
  view: SpaceView | null;
  ppu: number;
  screenArea: number;
  inkTarget: number;
  slots: number;
  previousDrawn: number;
};

let scratchLinks = new Float32Array(0);
let scratchColors = new Float32Array(0);
const scratchFor = (slots: number) => {
  if (scratchLinks.length < slots * 2) {
    scratchLinks = new Float32Array(slots * 2);
    scratchColors = new Float32Array(slots * 4);
  }
};

export class DetailWalk {
  private n = 0;
  private ink = 0;
  stopped: boolean;

  constructor(
    private readonly positions: Float32Array,
    private readonly postCounts: Uint32Array,
    private readonly inBase: Uint8Array,
    private readonly q: DetailView
  ) {
    scratchFor(q.slots);
    this.stopped = q.slots === 0;
  }

  level(tiles: EdgeRun[]): void {
    const { view } = this.q;
    if (!view || this.stopped) return;
    const { minX, minY, maxX, maxY } = view;
    const { positions, postCounts, inBase } = this;
    const inkCap = this.q.inkTarget * this.q.screenArea;
    const { ppu, slots } = this.q;
    const links = scratchLinks;
    const colors = scratchColors;
    let { n, ink } = this;
    const ranks = tiles.map((t) => t.index);
    const pos = new Uint32Array(tiles.length);
    for (;;) {
      if (n >= slots || ink >= inkCap) {
        this.stopped = true;
        break;
      }
      let best = -1;
      let rank = Infinity;
      for (let t = 0; t < ranks.length; t++) {
        const p = pos[t];
        if (p < ranks[t].length && ranks[t][p] < rank) {
          rank = ranks[t][p];
          best = t;
        }
      }
      if (best < 0) break;
      const tile = tiles[best];
      const p = pos[best]++;
      if (inBase[rank]) continue;
      const ea = tile.a[p];
      if (postCounts[ea] === 0) continue;
      const ax = positions[ea * 2];
      const ay = positions[ea * 2 + 1];
      if (ax < minX || ax > maxX || ay < minY || ay > maxY) continue;
      const eb = tile.b[p];
      if (postCounts[eb] === 0) continue;
      const bx = positions[eb * 2];
      const by = positions[eb * 2 + 1];
      if (bx < minX || bx > maxX || by < minY || by > maxY) continue;
      ink += Math.hypot((ax - bx) * ppu, (ay - by) * ppu);
      links[n * 2] = ea;
      links[n * 2 + 1] = eb;
      linkColorAt(colors, n, tile.weight[p]);
      n++;
    }
    this.n = n;
    this.ink = ink;
  }

  finish(): LinkDetail {
    const { n, ink, q } = this;
    const inkCap = q.inkTarget * q.screenArea;
    const written = Math.max(n, Math.min(q.previousDrawn, q.slots));
    parkRun(scratchLinks, n, written - n);
    return {
      links: scratchLinks.slice(0, written * 2),
      colors: scratchColors.slice(0, n * 4),
      drawn: n,
      clipped: n >= q.slots && ink < inkCap,
      ink: ink / q.screenArea
    };
  }
}

export type LinkForced = {
  links: Float32Array;
  colors: Float32Array;
  drawn: number;
  neighbors: Uint32Array;
};

export const selectForced = (
  shard: AdjacencyShard | null,
  first: number,
  node: number | null,
  previousDrawn: number,
  slots: number
): LinkForced => {
  const lo = shard && node !== null ? shard.off[node - first] : 0;
  const hi = shard && node !== null ? shard.off[node - first + 1] : 0;
  const n = hi - lo;
  const written = Math.max(n, Math.min(previousDrawn, slots));
  const links = new Float32Array(written * 2);
  const colors = new Float32Array(n * 4);
  const neighbors = new Uint32Array(node === null ? 0 : n + 1);
  if (shard && node !== null) {
    neighbors[0] = node;
    neighbors.set(shard.far.subarray(lo, hi), 1);
    for (let k = 0; k < n; k++) {
      links[k * 2] = node;
      links[k * 2 + 1] = shard.far[lo + k];
      linkColorAt(colors, k, shard.weight[lo + k]);
    }
  }
  parkRun(links, n, written - n);
  return { links, colors, drawn: n, neighbors };
};
