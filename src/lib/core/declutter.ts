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
const SNAPSHOT_FIRST = 25_000;

// placed labels live in one flat array, ENTRY floats each, chained per grid cell
// through ENTRY_NEXT with heads[cell] holding the first index
const ENTRY_X = 0;
const ENTRY_Y = 1;
const ENTRY_REACH = 2;
const ENTRY_HALF_H = 3;
const ENTRY_ZOOM = 4;
const ENTRY_NEXT = 5;
const ENTRY = 6;

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

// finds the zoom at which each label first clears every label placed before it; labels arrive in
// descending priority, so the earlier one always wins and one forward pass settles them all
export const labelZooms = (
  positions: Float32Array,
  reach: Float32Array,
  halfH: Float32Array,
  gapY: number,
  space: number,
  zoomMax: number,
  onSnapshot: (zooms: LabelZooms, done: number) => void
): LabelZooms => {
  const n = reach.length;
  let reachMax = 0;
  let halfMax = 0;
  for (let i = 0; i < n; i++) {
    if (reach[i] > reachMax) reachMax = reach[i];
    if (halfH[i] > halfMax) halfMax = halfH[i];
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
      // a conflict has to beat both zooms to matter, so search from the larger of the two
      const tq = Math.max(t, levelZoom[l]);
      // a label only extends right, so the columns are its own span; placed labels occupy one
      // row each, so the rows are widened by the tallest box instead
      const ry = (hi + halfMax + gapY) / tq;
      const row0 = Math.max(0, Math.floor((yi - ry) * invY[l]));
      const row1 = Math.min(rows[l] - 1, Math.floor((yi + ry) * invY[l]));
      const col0 = Math.min(cols[l] - 1, Math.max(0, Math.floor(xi * invX[l])));
      const col1 = Math.min(cols[l] - 1, Math.floor((xi + ri / tq) * invX[l]));
      for (let row = row0; row <= row1; row++) {
        const rowBase = base[l] + row * cols[l];
        for (let col = col0; col <= col1; col++)
          for (let e = heads[rowBase + col]; e >= 0;) {
            const o = e * ENTRY;
            const dx = xi - entry[o + ENTRY_X];
            const dy = yi - entry[o + ENTRY_Y];
            const ty = dy === 0 ? Infinity : (hi + entry[o + ENTRY_HALF_H] + gapY) / Math.abs(dy);
            const tx = dx > 0 ? entry[o + ENTRY_REACH] / dx : dx < 0 ? ri / -dx : Infinity;
            const tij = Math.min(tx, ty);
            if (tij > t && tij > entry[o + ENTRY_ZOOM]) t = tij;
            e = entry[o + ENTRY_NEXT];
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
      entry[o + ENTRY_X] = xi;
      entry[o + ENTRY_Y] = yi;
      entry[o + ENTRY_REACH] = ri;
      entry[o + ENTRY_HALF_H] = hi;
      entry[o + ENTRY_ZOOM] = t;
      entry[o + ENTRY_NEXT] = heads[cell];
      heads[cell] = entries++;
    }
  }

  return bucketed(zoom, reachMax);
};
