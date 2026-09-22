import { readTable, runs, values } from "$lib/core/table";

import { loadNodeText } from "./dataset";
import { fetchGz } from "./fetch";
import { WEIGHT_MAX, type Manifest } from "./manifest";

export type PathGraph = {
  nodes: Uint32Array;
  off: Int32Array;
  far: Uint32Array;
  weight: Uint16Array;
  row: Int32Array;
};

export type TagPath = { nodes: number[]; weights: number[] };

let graph: Promise<PathGraph> | null = null;

export const loadPathGraph = (manifest: Manifest): Promise<PathGraph> => {
  graph ??= fetchGz("paths.bin.gz").then((buffer) => {
    const t = readTable(buffer);
    const nodes = values<Uint32Array>(t, "node");
    const { off, values: far } = runs<Uint32Array>(t, "far");
    const weight = values<Uint16Array>(t, "weight");
    const row = new Int32Array(manifest.nodes).fill(-1);
    nodes.forEach((node, r) => {
      row[node] = r;
    });
    return { nodes, off, far, weight, row };
  });
  return graph;
};

class Heap {
  private keys = new Float64Array(1024);
  private vals = new Int32Array(1024);
  size = 0;

  push(val: number, key: number) {
    if (this.size === this.keys.length) {
      const keys = new Float64Array(this.size * 2);
      keys.set(this.keys);
      this.keys = keys;
      const vals = new Int32Array(this.size * 2);
      vals.set(this.vals);
      this.vals = vals;
    }
    let i = this.size++;
    while (i > 0) {
      const parent = (i - 1) >> 1;
      if (this.keys[parent] <= key) break;
      this.keys[i] = this.keys[parent];
      this.vals[i] = this.vals[parent];
      i = parent;
    }
    this.keys[i] = key;
    this.vals[i] = val;
  }

  pop(): [number, number] {
    const top: [number, number] = [this.vals[0], this.keys[0]];
    const key = this.keys[--this.size];
    const val = this.vals[this.size];
    let i = 0;
    for (;;) {
      let child = i * 2 + 1;
      if (child >= this.size) break;
      if (child + 1 < this.size && this.keys[child + 1] < this.keys[child]) child++;
      if (this.keys[child] >= key) break;
      this.keys[i] = this.keys[child];
      this.vals[i] = this.vals[child];
      i = child;
    }
    this.keys[i] = key;
    this.vals[i] = val;
    return top;
  }
}

const cost = (weight: number) => -Math.log(Math.max(weight, 1) / WEIGHT_MAX);

export const findPath = async (
  manifest: Manifest,
  from: number,
  to: number
): Promise<TagPath | null> => {
  const g = await loadPathGraph(manifest);
  const entries = async (node: number): Promise<[number, number][]> => {
    if (g.row[node] >= 0) return [[node, WEIGHT_MAX]];
    const e = (await loadNodeText(manifest, node))?.edges ?? [];
    const out: [number, number][] = [];
    for (let k = 0; k + 2 < e.length; k += 3) if (g.row[e[k]] >= 0) out.push([e[k], e[k + 1]]);
    return out;
  };
  const [starts, ends] = await Promise.all([entries(from), entries(to)]);
  if (starts.length === 0 || ends.length === 0) return null;
  const k = g.nodes.length;
  const dist = new Float64Array(k).fill(Infinity);
  const pred = new Int32Array(k).fill(-1);
  const via = new Uint16Array(k);
  const done = new Uint8Array(k);
  const heap = new Heap();
  for (const [node, w] of starts) {
    const r = g.row[node];
    const d = node === from ? 0 : cost(w);
    if (d < dist[r]) {
      dist[r] = d;
      via[r] = w;
      heap.push(r, d);
    }
  }
  while (heap.size > 0) {
    const [r, d] = heap.pop();
    if (done[r]) continue;
    done[r] = 1;
    for (let e = g.off[r]; e < g.off[r + 1]; e++) {
      const s = g.row[g.far[e]];
      const nd = d + cost(g.weight[e]);
      if (nd < dist[s]) {
        dist[s] = nd;
        pred[s] = r;
        via[s] = g.weight[e];
        heap.push(s, nd);
      }
    }
  }
  let best = -1;
  let bestTotal = Infinity;
  let exit = WEIGHT_MAX;
  for (const [node, w] of ends) {
    const r = g.row[node];
    const total = dist[r] + (node === to ? 0 : cost(w));
    if (total < bestTotal) {
      bestTotal = total;
      best = r;
      exit = w;
    }
  }
  if (best < 0) return null;
  const rows: number[] = [];
  for (let r = best; r >= 0; r = pred[r]) rows.push(r);
  rows.reverse();
  const nodes = rows.map((r) => g.nodes[r]);
  const weights = rows.slice(1).map((r) => via[r] / WEIGHT_MAX);
  if (nodes[0] !== from) {
    nodes.unshift(from);
    weights.unshift(via[rows[0]] / WEIGHT_MAX);
  }
  if (nodes[nodes.length - 1] !== to) {
    nodes.push(to);
    weights.push(exit / WEIGHT_MAX);
  }
  return { nodes, weights };
};
