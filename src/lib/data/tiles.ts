import { Cursor } from "$lib/core/binary";

import { fetchGz } from "./fetch";

// a run of edges picked out of the global edge list by rank; written by edge_run in the
// pipeline's emit.rs, and what both base.bin.gz and the tile files hold
export type EdgeRun = { index: Uint32Array; a: Uint32Array; b: Uint32Array; weight: Uint16Array };

export type AdjacencyShard = { off: Uint32Array; far: Uint32Array; weight: Uint16Array };

const EDGE_RUN_STRIDE = 3 * Uint32Array.BYTES_PER_ELEMENT + Uint16Array.BYTES_PER_ELEMENT;

export const readEdgeRun = (c: Cursor, n: number): EdgeRun => ({
  index: c.u32(n),
  a: c.u32(n),
  b: c.u32(n),
  weight: c.u16(n)
});

export const loadTile = async (name: string): Promise<EdgeRun> => {
  const buffer = await fetchGz(name);
  return readEdgeRun(new Cursor(buffer), buffer.byteLength / EDGE_RUN_STRIDE);
};

export const loadAdjacencyShard = async (
  shard: number,
  shardSize: number,
  nodes: number
): Promise<AdjacencyShard> => {
  const k = Math.min(shardSize, nodes - shard * shardSize);
  const c = new Cursor(await fetchGz(`adj/${shard}.bin.gz`));
  const off = c.u32(k + 1);
  const m = off[k];
  return { off, far: c.u32(m), weight: c.u16(m) };
};
