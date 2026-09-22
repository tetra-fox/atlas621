import { setDataVersion } from "$lib/data/fetch";
import { loadAdjacencyShard, loadTile, type AdjacencyShard, type EdgeRun } from "$lib/data/tiles";

import {
  baseMembership,
  buildScene,
  DetailWalk,
  selectBase,
  selectForced,
  type DetailView,
  type LinkBase,
  type LinkDetail,
  type LinkForced,
  type LinkInput,
  type LinkQuery,
  type LinkScene,
  type SpaceView
} from "./linkselect";

export type LinksInit = { type: "init"; data: LinkInput };
export type SceneRequest = { type: "scene"; id: number; query: LinkQuery };
export type BaseRequest = {
  type: "base";
  id: number;
  shown: number;
  previousShown: number;
};
export type DetailRequest = { type: "detail"; id: number; view: DetailView };
export type ForcedRequest = {
  type: "forced";
  id: number;
  node: number | null;
  previousDrawn: number;
};
export type LinksRequest = SceneRequest | BaseRequest | DetailRequest | ForcedRequest;
export type LinksMessage = LinksInit | LinksRequest;
export type SceneResponse = LinkScene & { type: "scene"; id: number; selectMs: number };
export type BaseResponse = LinkBase & { type: "base"; id: number };
export type DetailResponse = LinkDetail & {
  type: "detail";
  id: number;
  selectMs: number;
  fetchMs: number;
  levels: number;
};
export type ForcedResponse = LinkForced & {
  type: "forced";
  id: number;
  node: number | null;
  selectMs: number;
  fetchMs: number;
};
export type ErrorResponse = { type: "error"; message: string };
export type LinksResponse =
  | SceneResponse
  | BaseResponse
  | DetailResponse
  | ForcedResponse
  | ErrorResponse;

let data: LinkInput | null = null;
let inBase: Uint8Array | null = null;
let tileFiles = new Set<string>();
const tiles = new Map<string, Promise<EdgeRun>>();
const shards = new Map<number, Promise<AdjacencyShard>>();
let latestDetail = 0;
let latestForced = 0;

const post = (message: LinksResponse, transfer: Transferable[] = []) =>
  (self as unknown as Worker).postMessage(message, transfer);
const failed = (e: unknown) =>
  post({ type: "error", message: e instanceof Error ? e.message : String(e) });

const tileAt = (name: string): Promise<EdgeRun> => {
  let p = tiles.get(name);
  if (!p) {
    p = loadTile(name);
    tiles.set(name, p);
  }
  return p;
};

const shardAt = (shard: number): Promise<AdjacencyShard> => {
  let p = shards.get(shard);
  if (!p) {
    p = loadAdjacencyShard(shard);
    shards.set(shard, p);
  }
  return p;
};

const tilesFor = (d: LinkInput, level: number, view: SpaceView): Promise<EdgeRun>[] => {
  const cells = 1 << level;
  const at = (v: number) => Math.min(cells - 1, Math.max(0, Math.floor((v / d.space) * cells)));
  const out: Promise<EdgeRun>[] = [];
  for (let y = at(view.minY); y <= at(view.maxY); y++)
    for (let x = at(view.minX); x <= at(view.maxX); x++) {
      const name = `tiles/${level}/${x}_${y}.bin.gz`;
      if (tileFiles.has(name)) out.push(tileAt(name));
    }
  return out;
};

const detail = async (msg: DetailRequest, d: LinkInput, base: Uint8Array) => {
  const walk = new DetailWalk(d.positions, d.postCounts, base, msg.view);
  const { view } = msg.view;
  const last = d.tileCutoffs.length;
  let fetchMs = 0;
  let selectMs = 0;
  let levels = 0;
  while (view && !walk.stopped && levels <= last) {
    const t0 = performance.now();
    const tiles = await Promise.all(tilesFor(d, levels, view));
    fetchMs += performance.now() - t0;
    if (msg.id !== latestDetail) return;
    const t1 = performance.now();
    walk.level(tiles);
    selectMs += performance.now() - t1;
    levels++;
  }
  const reply: DetailResponse = {
    type: "detail",
    id: msg.id,
    selectMs,
    fetchMs,
    levels,
    ...walk.finish()
  };
  post(reply, [reply.links.buffer, reply.colors.buffer]);
};

const forced = async (msg: ForcedRequest, d: LinkInput) => {
  let shard: AdjacencyShard | null = null;
  let first = 0;
  let fetchMs = 0;
  if (msg.node !== null) {
    const s = Math.floor(msg.node / d.adjShardSize);
    first = s * d.adjShardSize;
    const t0 = performance.now();
    shard = await shardAt(s);
    fetchMs = performance.now() - t0;
    if (msg.id !== latestForced) return;
  }
  const t1 = performance.now();
  const out = selectForced(shard, first, msg.node, msg.previousDrawn, d.maxDegree);
  const reply: ForcedResponse = {
    type: "forced",
    id: msg.id,
    node: msg.node,
    selectMs: performance.now() - t1,
    fetchMs,
    ...out
  };
  post(reply, [reply.links.buffer, reply.colors.buffer, reply.neighbors.buffer]);
};

self.onmessage = (event: MessageEvent<LinksMessage>) => {
  const msg = event.data;
  if (msg.type === "init") {
    data = msg.data;
    inBase = baseMembership(msg.data.base, msg.data.edgeCount);
    tileFiles = new Set(msg.data.tiles);
    setDataVersion(msg.data.version);
    const first = "tiles/0/0_0.bin.gz";
    if (tileFiles.has(first)) tileAt(first).catch(failed);
    return;
  }
  if (!data || !inBase) return;
  if (msg.type === "scene") {
    const t0 = performance.now();
    const s = buildScene(data, msg.query);
    const reply: SceneResponse = {
      type: "scene",
      id: msg.id,
      selectMs: performance.now() - t0,
      ...s
    };
    const transfer: Transferable[] = [reply.links.buffer, reply.linkColors.buffer];
    if (reply.styles) transfer.push(reply.styles.buffer);
    if (reply.arrows) transfer.push(reply.arrows.buffer);
    post(reply, transfer);
    return;
  }
  if (msg.type === "base") {
    const reply: BaseResponse = {
      type: "base",
      id: msg.id,
      ...selectBase(data.base, msg.shown, msg.previousShown)
    };
    post(reply, [reply.links.buffer]);
    return;
  }
  if (msg.type === "detail") {
    latestDetail = msg.id;
    detail(msg, data, inBase).catch(failed);
    return;
  }
  latestForced = msg.id;
  forced(msg, data).catch(failed);
};
