import type { Years } from "$lib/data/dataset";

import { CATEGORY_COLORS, radiusFor, yearColor } from "./palette";

export type Look = {
  colorMode: "category" | "birth";
  yearExtent: [number, number];
};

export const visibleSizes = (
  counts: Uint32Array,
  categories: Uint8Array,
  categoryMask: number
): Float32Array => {
  const out = new Float32Array(counts.length);
  for (let i = 0; i < counts.length; i++) {
    const on = ((categoryMask >> categories[i]) & 1) === 1;
    out[i] = on && counts[i] > 0 ? radiusFor(counts[i]) : 0;
  }
  return out;
};

export const pointColors = (
  categories: Uint8Array,
  firstYear: Uint16Array | null,
  look: Look,
  sizes: Float32Array
): Float32Array => {
  const out = new Float32Array(categories.length * 4);
  for (let i = 0; i < categories.length; i++) {
    const [r, g, b] =
      look.colorMode === "birth" && firstYear
        ? yearColor(firstYear[i], look.yearExtent[0], look.yearExtent[1])
        : CATEGORY_COLORS[categories[i]];
    out[i * 4] = r / 255;
    out[i * 4 + 1] = g / 255;
    out[i * 4 + 2] = b / 255;
    out[i * 4 + 3] = sizes[i] > 0 ? 0.9 : 0;
  }
  return out;
};

export const countsWithin = (
  postCounts: Uint32Array,
  years: Years | null,
  yearList: number[],
  range: [number, number] | null
): Uint32Array => {
  if (range === null || !years) return postCounts;
  const n = postCounts.length;
  const span = yearList.length;
  const from = yearList.indexOf(range[0]);
  const to = yearList.indexOf(range[1]);
  const out = new Uint32Array(n);
  if (from < 0 || to < from) return out;
  const { rows, over } = years;
  for (let i = 0; i < n; i++) {
    let sum = 0;
    const base = i * span;
    for (let y = from; y <= to; y++) sum += rows[base + y];
    out[i] = sum;
  }
  for (let k = 0; k < over.length; k += 2) {
    const y = over[k] % span;
    if (y >= from && y <= to) out[Math.floor(over[k] / span)] += over[k + 1] - 65535;
  }
  return out;
};

export const yearRow = (years: Years, span: number, node: number): number[] => {
  const row = Array.from(years.rows.subarray(node * span, (node + 1) * span));
  const { over } = years;
  for (let k = 0; k < over.length; k += 2) {
    const y = over[k] - node * span;
    if (y >= 0 && y < span) row[y] = over[k + 1];
  }
  return row;
};

export const lastDayOfYear = (year: number): number =>
  Math.round((Date.UTC(year, 11, 31) - Date.UTC(2007, 0, 1)) / 86_400_000);
