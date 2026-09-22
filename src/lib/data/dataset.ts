import { Names } from "$lib/core/strings";
import { readTable, runs, strings, values } from "$lib/core/table";

import { fetchColumn, fetchGz, fetchJson, setDataVersion } from "./fetch";
import type { Manifest } from "./manifest";
import NamesWorker from "./names.worker?worker";
import type { NamesRequest, NamesResponse } from "./names.worker";
import { beginStep, endStep } from "./progress";
import { readEdgeRun, type EdgeRun } from "./tiles";

export type Territory = {
  community: number;
  label: boolean;
  anchor: [number, number];
  cells: number;
  hexes: number[];
};

export type TerritoryLevel = { hex_size: number; origin: [number, number]; features: Territory[] };
export type Territories = { regions: TerritoryLevel; continents: TerritoryLevel };

export type Community = { name: string; tags: string[]; size: number };

export type Communities = {
  regions: Community[];
  continents: Community[];
  continent_of_region: number[];
};

export type Implication = [child: number, parent: number, day: number | null, approx: boolean];

export type HistoryEvent = { d: string; k: string; o: string; s: string; a?: boolean };

export type NodeText = {
  w?: string;
  l?: number[];
  a?: string[];
  c?: string[];
  p?: string[];
  h?: HistoryEvent[];
  r?: [number, number, number];
  g?: number;
  e?: number[];
  s?: number[];
};

export type SearchEntry = {
  name: string;
  postCount: number;
  category: number;
  // the node this row points at, or -1 for a tag that never became one
  node: number;
  tail: number[];
  // set only on an alias row, naming the tag it redirects to
  alias?: string;
};

export type ChangelogEntry = {
  id: number;
  date: string;
  title: string;
  ops: [string, string, string][];
};

export type Core = {
  manifest: Manifest;
  positions: Float32Array;
  postCounts: Uint32Array;
  categories: Uint8Array;
};

export const loadManifest = async (): Promise<Manifest> => {
  beginStep("fetching the file list");
  const manifest = await fetchJson<Manifest>("manifest.json");
  endStep("fetching the file list");
  setDataVersion(manifest.generated_at);
  return manifest;
};

const ask = <Request, Response>(
  worker: Worker,
  request: Request,
  transfer: Transferable[]
): Promise<Response> =>
  new Promise((resolve, reject) => {
    worker.onmessage = (event: MessageEvent<Response>) => {
      worker.terminate();
      resolve(event.data);
    };
    worker.onerror = (event) => {
      worker.terminate();
      reject(new Error(event.message));
    };
    worker.postMessage(request, transfer);
  });

export const loadCore = async (manifest: Manifest): Promise<Core> => {
  const [positions, postCounts, categories] = await Promise.all([
    fetchColumn<Float32Array>(manifest, "positions.bin.gz", "position", "tag positions"),
    fetchColumn<Uint32Array>(manifest, "post_counts.bin.gz", "post_counts", "post counts"),
    fetchColumn<Uint8Array>(manifest, "categories.bin.gz", "categories", "tag categories")
  ]);
  return { manifest, positions, postCounts, categories };
};

export const loadBase = async (manifest: Manifest): Promise<EdgeRun> => {
  return readEdgeRun(await fetchGz("base.bin.gz", manifest.files["base.bin.gz"], "first edges"));
};

export const loadNames = async (manifest: Manifest): Promise<Names> => {
  const count = manifest.nodes;
  const buffer = await fetchGz("names.bin.gz", manifest.files["names.bin.gz"], "tags");
  beginStep("indexing tags");
  const request: NamesRequest = { count, buffer };
  const back = await ask<NamesRequest, NamesResponse>(new NamesWorker(), request, [buffer]);
  endStep("indexing tags");
  const { offsets, bytes } = strings(readTable(back.buffer), "name");
  return new Names(offsets, bytes, back.table);
};

export const loadU16 = (
  manifest: Manifest,
  file: string,
  column: string,
  label: string
): Promise<Uint16Array> => fetchColumn<Uint16Array>(manifest, file, column, label);

export type Years = { rows: Uint16Array; over: Uint32Array };

export const loadYears = async (manifest: Manifest): Promise<Years> => {
  const [rows, over] = await Promise.all([
    fetchColumn<Uint16Array>(manifest, "years.bin.gz", "years"),
    fetchColumn<Uint32Array>(manifest, "years_over.bin.gz", "years_over")
  ]);
  return { rows, over };
};

export const loadTerritories = (manifest: Manifest) =>
  fetchJson<Territories>("territories.json", manifest.files["territories.json"], "island tiles");
export const loadCommunities = (manifest: Manifest) =>
  fetchJson<Communities>("communities.json", manifest.files["communities.json"], "island names");
export const loadImplications = (manifest: Manifest) =>
  fetchJson<Implication[]>(
    "implications.json",
    manifest.files["implications.json"],
    "implications"
  );
export const loadChangelog = () => fetchJson<ChangelogEntry[]>("changelog.json");

const textCache = new Map<number, Promise<Record<string, NodeText>>>();

const textShardOf = (starts: number[], node: number): number => {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= node) lo = mid;
    else hi = mid - 1;
  }
  return lo;
};

export const loadNodeText = async (
  manifest: Manifest,
  node: number
): Promise<NodeText | undefined> => {
  const shard = textShardOf(manifest.text_shards, node);
  let p = textCache.get(shard);
  if (!p) {
    p = fetchJson<Record<string, NodeText>>(`text/${shard}.json`);
    textCache.set(shard, p);
  }
  return (await p)[String(node)];
};

// must agree with shard_key in the pipeline's search.rs, which named the shard files: the first
// two code points, not the first two utf-16 units
export const shardKey = (query: string): string => {
  let key = "";
  for (const ch of query) {
    if (key.length === 2) break;
    key += /^[a-z0-9]$/.test(ch) ? ch : "_";
  }
  return key.padEnd(2, "_");
};

const searchCache = new Map<string, Promise<SearchEntry[]>>();

export const loadSearchShard = (key: string): Promise<SearchEntry[]> => {
  let p = searchCache.get(key);
  if (!p) {
    p = fetchGz(`search/${key}.bin.gz`)
      .then((buffer) => {
        const t = readTable(buffer);
        const name = t.getChild("name");
        const alias = t.getChild("alias");
        const postCount = values<Uint32Array>(t, "post_count");
        const category = values<Uint8Array>(t, "category");
        const node = values<Int32Array>(t, "node");
        const tail = runs<Uint32Array>(t, "tail");
        return Array.from({ length: t.numRows }, (_, r) => ({
          name: name?.get(r) ?? "",
          postCount: postCount[r],
          category: category[r],
          node: node[r],
          tail: Array.from(tail.values.subarray(tail.off[r], tail.off[r + 1])),
          alias: alias?.get(r) ?? undefined
        }));
      })
      // no shard means no tag starts with those two characters
      .catch(() => []);
    searchCache.set(key, p);
  }
  return p;
};
