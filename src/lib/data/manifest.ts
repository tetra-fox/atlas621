// edge weights ship as u16 over 0..1; matches quantize() in the pipeline's emit.rs
export const WEIGHT_MAX = 65535;

export type Manifest = {
  export_date: string;
  generated_at: string;
  node_floor: number;
  playground_floor: number;
  nodes: number;
  edges: number;
  space_size: number;
  max_degree: number;
  base_links: number;
  tail_neighbors: number;
  text_shards: number[];
  tile_cutoffs: number[];
  adj_shard_size: number;
  years: number[];
  years_over: number;
  files: Record<string, number>;
  parts: Record<string, number>;
};
