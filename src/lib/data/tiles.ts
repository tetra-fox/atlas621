import { readTable, runs, values } from "$lib/core/table";

import { fetchGz } from "./fetch";

// a run of edges picked out of the global edge list by rank; written by edge_run in the
// pipeline's emit.rs, and what both base.bin.gz and the tile files hold
export type EdgeRun = { index: Uint32Array; a: Uint32Array; b: Uint32Array; weight: Uint16Array };

export type AdjacencyShard = { off: Int32Array; far: Uint32Array; weight: Uint16Array };

export const readEdgeRun = (buffer: ArrayBuffer): EdgeRun => {
  const t = readTable(buffer);
  return {
    index: values<Uint32Array>(t, "rank"),
    a: values<Uint32Array>(t, "a"),
    b: values<Uint32Array>(t, "b"),
    weight: values<Uint16Array>(t, "weight")
  };
};

export const loadTile = async (name: string): Promise<EdgeRun> => readEdgeRun(await fetchGz(name));

export const loadAdjacencyShard = async (shard: number): Promise<AdjacencyShard> => {
  const t = readTable(await fetchGz(`adj/${shard}.bin.gz`));
  const far = runs<Uint32Array>(t, "far");
  return { off: far.off, far: far.values, weight: values<Uint16Array>(t, "weight") };
};
