import { radiusFor } from "./palette";

export const LABEL_PAD = 4;
export const LABEL_GAP_X = 24;
export const LABEL_GAP_Y = 16;
export const LABEL_LEFT = 3;
export const LABEL_SIZES = [10, 11, 12, 13, 14, 15] as const;

export const labelFontSize = (count: number): number =>
  Math.round(Math.min(15, 10 + Math.log10(Math.max(1, count))));
export const labelBoxHeight = (size: number): number => Math.ceil(size * 1.4) + LABEL_PAD * 2;
export const LABEL_HALF_MAX = labelBoxHeight(15) / 2;

const LEVEL_MIN = -7;
const LEVEL_MAX = 6;
const BUCKETS_PER_LEVEL = 32;
export const ZOOM_BUCKETS = (LEVEL_MAX + 1 - LEVEL_MIN) * BUCKETS_PER_LEVEL;

export const zoomBucket = (zoom: number): number => {
  if (zoom === Infinity) return ZOOM_BUCKETS;
  if (zoom <= 0) return 0;
  const b = Math.floor((Math.log2(zoom) - LEVEL_MIN) * BUCKETS_PER_LEVEL);
  return Math.min(ZOOM_BUCKETS - 1, Math.max(0, b));
};

export type LabelZooms = {
  zoom: Float32Array;
  order: Uint32Array;
  starts: Uint32Array;
  reach: number;
};

const CELL_X = 160;
const CELL_Y = 80;
const CELL_MAX = 32;
const ENTRY = 6;
const SNAPSHOT_FIRST = 25_000;

const bucketed = (zoom: Float32Array, reach: number): LabelZooms => {
  const n = zoom.length;
  const starts = new Uint32Array(ZOOM_BUCKETS + 2);
  for (let i = 0; i < n; i++) starts[zoomBucket(zoom[i]) + 1]++;
  for (let b = 0; b <= ZOOM_BUCKETS; b++) starts[b + 1] += starts[b];
  const fill = starts.slice(0, ZOOM_BUCKETS + 1);
  const order = new Uint32Array(n);
  for (let i = 0; i < n; i++) order[fill[zoomBucket(zoom[i])]++] = i;
  return { zoom, order, starts, reach };
};

export const labelZooms = (
  positions: Float32Array,
  postCounts: Uint32Array,
  widths: Float32Array,
  space: number,
  zoomMax: number,
  onSnapshot: (zooms: LabelZooms, done: number) => void
): LabelZooms => {
  const n = postCounts.length;
  const reach = new Float32Array(n);
  const halfH = new Float32Array(n);
  let reachMax = 0;
  for (let i = 0; i < n; i++) {
    reach[i] = 2 * radiusFor(postCounts[i]) + widths[i] + LABEL_GAP_X;
    halfH[i] = labelBoxHeight(labelFontSize(postCounts[i])) / 2;
    if (reach[i] > reachMax) reachMax = reach[i];
  }

  const levels = LEVEL_MAX + 1 - LEVEL_MIN;
  const levelZoom = new Float64Array(levels);
  const cols = new Int32Array(levels);
  const rows = new Int32Array(levels);
  const invX = new Float64Array(levels);
  const invY = new Float64Array(levels);
  const base = new Int32Array(levels + 1);
  for (let l = 0; l < levels; l++) {
    levelZoom[l] = 2 ** (l + LEVEL_MIN);
    const cellX = Math.min(CELL_MAX, CELL_X / levelZoom[l]);
    const cellY = Math.min(CELL_MAX, CELL_Y / levelZoom[l]);
    cols[l] = Math.ceil(space / cellX);
    rows[l] = Math.ceil(space / cellY);
    invX[l] = 1 / cellX;
    invY[l] = 1 / cellY;
    base[l + 1] = base[l] + cols[l] * rows[l];
  }
  const heads = new Int32Array(base[levels]).fill(-1);
  const members = new Int32Array(levels);
  let capacity = n + (n >> 2);
  let entry = new Float32Array(capacity * ENTRY);
  let entries = 0;

  const zoom = new Float32Array(n).fill(Infinity);
  let snapshotAt = SNAPSHOT_FIRST;
  for (let i = 0; i < n; i++) {
    if (i === snapshotAt) {
      onSnapshot(bucketed(zoom.slice(), reachMax), i);
      snapshotAt *= 2;
    }
    const xi = positions[i * 2];
    const yi = positions[i * 2 + 1];
    const ri = reach[i];
    const hi = halfH[i];
    let t = 0;
    for (let l = levels - 1; l >= 0 && t <= zoomMax; l--) {
      if (members[l] === 0) continue;
      const tq = Math.max(t, levelZoom[l]);
      const ry = (hi + LABEL_HALF_MAX + LABEL_GAP_Y) / tq;
      const row0 = Math.max(0, Math.floor((yi - ry) * invY[l]));
      const row1 = Math.min(rows[l] - 1, Math.floor((yi + ry) * invY[l]));
      const col0 = Math.min(cols[l] - 1, Math.max(0, Math.floor(xi * invX[l])));
      const col1 = Math.min(cols[l] - 1, Math.floor((xi + ri / tq) * invX[l]));
      for (let row = row0; row <= row1; row++) {
        const rowBase = base[l] + row * cols[l];
        for (let col = col0; col <= col1; col++)
          for (let e = heads[rowBase + col]; e >= 0;) {
            const o = e * ENTRY;
            const dx = xi - entry[o];
            const dy = yi - entry[o + 1];
            const ty = dy === 0 ? Infinity : (hi + entry[o + 3] + LABEL_GAP_Y) / Math.abs(dy);
            const tx = dx > 0 ? entry[o + 2] / dx : dx < 0 ? ri / -dx : Infinity;
            const tij = Math.min(tx, ty);
            if (tij > t && tij > entry[o + 4]) t = tij;
            e = entry[o + 5];
          }
      }
    }
    if (t > zoomMax) {
      zoom[i] = Infinity;
      continue;
    }
    zoom[i] = t;
    const l = t === 0 ? 0 : Math.min(levels - 1, Math.max(0, Math.floor(Math.log2(t)) - LEVEL_MIN));
    members[l]++;
    const zq = Math.max(t, levelZoom[l]);
    const row = Math.min(rows[l] - 1, Math.max(0, Math.floor(yi * invY[l])));
    const col0 = Math.min(cols[l] - 1, Math.max(0, Math.floor(xi * invX[l])));
    const col1 = Math.min(cols[l] - 1, Math.floor((xi + ri / zq) * invX[l]));
    for (let col = col0; col <= col1; col++) {
      if (entries === capacity) {
        capacity += capacity >> 1;
        const grown = new Float32Array(capacity * ENTRY);
        grown.set(entry);
        entry = grown;
      }
      const cell = base[l] + row * cols[l] + col;
      const o = entries * ENTRY;
      entry[o] = xi;
      entry[o + 1] = yi;
      entry[o + 2] = ri;
      entry[o + 3] = hi;
      entry[o + 4] = t;
      entry[o + 5] = heads[cell];
      heads[cell] = entries++;
    }
  }

  return bucketed(zoom, reachMax);
};
