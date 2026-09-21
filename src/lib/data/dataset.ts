import type { LabelZooms } from "$lib/map/labels";
import LabelsWorker from "$lib/map/labels.worker?worker";
import type { LabelsRequest, LabelsResponse } from "$lib/map/labels.worker";

import {
  beginStep,
  Cursor,
  dataUrl,
  endStep,
  fetchArray,
  fetchGz,
  fetchJson,
  setDataVersion,
  type Manifest
} from "./format";
import { Names } from "./names";
import NamesWorker from "./names.worker?worker";
import type { NamesRequest, NamesResponse } from "./names.worker";

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

export type SearchEntry = { n: string; c: number; k: number; i: number; t?: number[]; a?: string };

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

export type Base = {
  index: Uint32Array;
  a: Uint32Array;
  b: Uint32Array;
  weight: Uint16Array;
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
  const n = manifest.nodes;
  const [positions, postCounts, categories] = await Promise.all([
    fetchArray(manifest, "positions.bin.gz", "tag positions").then((b) => new Cursor(b).f32(n * 2)),
    fetchArray(manifest, "post_counts.bin.gz", "post counts").then((b) => new Cursor(b).u32(n)),
    fetchArray(manifest, "categories.bin.gz", "tag categories").then((b) => new Cursor(b).u8(n))
  ]);
  return { manifest, positions, postCounts, categories };
};

export const loadBase = async (manifest: Manifest): Promise<Base> => {
  const k = manifest.base_links;
  const c = new Cursor(await fetchGz("base.bin.gz", manifest.files["base.bin.gz"], "first edges"));
  return { index: c.u32(k), a: c.u32(k), b: c.u32(k), weight: c.u16(k) };
};

export const loadNames = async (manifest: Manifest): Promise<Names> => {
  const count = manifest.nodes;
  const buffer = await fetchArray(manifest, "names.bin.gz", "tags");
  beginStep("indexing tags");
  const request: NamesRequest = { count, buffer };
  const back = await ask<NamesRequest, NamesResponse>(new NamesWorker(), request, [buffer]);
  endStep("indexing tags");
  const c = new Cursor(back.buffer);
  return new Names(c.u32(count + 1), c.u8(c.remaining), back.table);
};

export const loadLabelZooms = (
  core: Core,
  names: Names,
  font: string,
  zoomMax: number,
  onSnapshot: (zooms: LabelZooms) => void
): Promise<LabelZooms> =>
  new Promise((resolve, reject) => {
    const worker = new LabelsWorker();
    const request: LabelsRequest = {
      positions: core.positions.slice(),
      postCounts: core.postCounts.slice(),
      offsets: names.offsets.slice(),
      bytes: names.bytes.slice(),
      font,
      space: core.manifest.space_size,
      zoomMax
    };
    worker.onmessage = (event: MessageEvent<LabelsResponse>) => {
      const { done, ...zooms } = event.data;
      onSnapshot(zooms);
      if (done === zooms.zoom.length) {
        worker.terminate();
        resolve(zooms);
      }
    };
    worker.onerror = (event) => {
      worker.terminate();
      reject(new Error(event.message));
    };
    worker.postMessage(request, [
      request.positions.buffer,
      request.postCounts.buffer,
      request.offsets.buffer,
      request.bytes.buffer
    ]);
  });

export const loadU16 = async (
  manifest: Manifest,
  file: string,
  n: number,
  label: string
): Promise<Uint16Array> => new Cursor(await fetchArray(manifest, file, label)).u16(n);

export type Years = { rows: Uint16Array; over: Uint32Array };

export const loadYears = async (manifest: Manifest): Promise<Years> => {
  const [rows, over] = await Promise.all([
    fetchArray(manifest, "years.bin.gz").then((b) =>
      new Cursor(b).u16(manifest.nodes * manifest.years.length)
    ),
    fetchArray(manifest, "years_over.bin.gz").then((b) =>
      new Cursor(b).u32(manifest.years_over * 2)
    )
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

export const shardKey = (query: string): string => {
  let key = "";
  for (const ch of (query + "__").slice(0, 2)) key += /[a-z0-9]/.test(ch) ? ch : "_";
  return key;
};

const searchCache = new Map<string, Promise<SearchEntry[]>>();

export const loadSearchShard = (key: string): Promise<SearchEntry[]> => {
  let p = searchCache.get(key);
  if (!p) {
    p = fetch(dataUrl(`search/${key}.json`)).then((res) =>
      res.ok ? (res.json() as Promise<SearchEntry[]>) : []
    );
    searchCache.set(key, p);
  }
  return p;
};
