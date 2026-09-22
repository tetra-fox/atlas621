import type { Names } from "$lib/core/strings";
import uFuzzy from "@leeoniya/ufuzzy";

import { loadSearchShard, shardKey, type SearchEntry } from "./dataset";

const fuzzy = new uFuzzy({ intraMode: 1, intraIns: 1, intraSub: 1, intraTrn: 1, intraDel: 1 });

export type SearchResult = SearchEntry & { prefix: boolean };

export const topByCount = (
  names: Names,
  postCounts: Uint32Array,
  categories: Uint8Array,
  limit = 30
): SearchResult[] =>
  Array.from({ length: Math.min(limit, names.length) }, (_, i) => ({
    name: names.at(i),
    postCount: postCounts[i],
    category: categories[i],
    node: i,
    tail: [],
    prefix: false
  }));

export const search = async (rawQuery: string, limit = 30): Promise<SearchResult[]> => {
  const query = rawQuery.trim().toLowerCase().replace(/\s+/g, "_");
  if (query.length === 0) return [];
  const entries = await loadSearchShard(shardKey(query));
  if (entries.length === 0) return [];
  const names = entries.map((e) => e.name);
  const seen = new Set<number>();
  const out: SearchResult[] = [];
  const take = (idx: number, prefix: boolean) => {
    if (seen.has(idx)) return;
    seen.add(idx);
    out.push({ ...entries[idx], prefix });
  };
  let lo = 0;
  let hi = names.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (names[mid] < query) lo = mid + 1;
    else hi = mid;
  }
  const prefixed: number[] = [];
  for (let i = lo; i < names.length && names[i].startsWith(query); i++) prefixed.push(i);
  prefixed.sort((a, b) => entries[b].postCount - entries[a].postCount);
  for (const i of prefixed.slice(0, limit)) take(i, true);
  if (out.length < limit && query.length >= 3) {
    const idxs = fuzzy.filter(names, query);
    if (idxs && idxs.length > 0) {
      const ranked =
        idxs.length <= 2000
          ? fuzzy.sort(fuzzy.info(idxs, names, query), names, query).map((o) => idxs[o])
          : idxs;
      const byCount = [...ranked].sort((a, b) => entries[b].postCount - entries[a].postCount);
      for (const i of byCount) {
        if (out.length >= limit) break;
        take(i, false);
      }
    }
  }
  return out;
};

export const compactCount = (n: number): { value: number; fraction: number; suffix: string } =>
  n >= 1_000_000
    ? { value: n / 1_000_000, fraction: 1, suffix: "M" }
    : n >= 10_000
      ? { value: Math.round(n / 1000), fraction: 0, suffix: "k" }
      : n >= 1000
        ? { value: n / 1000, fraction: 1, suffix: "k" }
        : { value: n, fraction: 0, suffix: "" };

export const formatCount = (n: number): string => {
  const { value, fraction, suffix } = compactCount(n);
  return `${value.toFixed(fraction)}${suffix}`;
};
