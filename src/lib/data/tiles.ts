import { Cursor, fetchGz } from "./format";

export type Tile = { index: Uint32Array; a: Uint32Array; b: Uint32Array; weight: Uint16Array };

export type AdjacencyShard = { off: Uint32Array; far: Uint32Array; weight: Uint16Array };

const TILE_STRIDE = 14;

export const loadTile = async (name: string): Promise<Tile> => {
  const buffer = await fetchGz(name);
  const n = buffer.byteLength / TILE_STRIDE;
  const c = new Cursor(buffer);
  return { index: c.u32(n), a: c.u32(n), b: c.u32(n), weight: c.u16(n) };
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
