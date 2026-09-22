import { Cursor } from "$lib/core/binary";

import { fetchGz } from "./fetch";
import type { Manifest } from "./manifest";

export type Vectors = { dim: number; nodes: Uint32Array; data: Int8Array; row: Int32Array };

export type Term = { sign: 1 | -1; nodes: number[] };

export type Hit = { node: number; score: number };

let loading: Promise<Vectors> | null = null;

export const loadVectors = (manifest: Manifest): Promise<Vectors> => {
  loading ??= fetchGz("vectors.bin.gz").then((buffer) => {
    const c = new Cursor(buffer);
    const [k, dim] = c.u32(2);
    const nodes = c.u32(k);
    const bytes = c.u8(k * dim);
    const data = new Int8Array(bytes.buffer, bytes.byteOffset, bytes.length);
    const row = new Int32Array(manifest.nodes).fill(-1);
    nodes.forEach((node, r) => {
      row[node] = r;
    });
    return { dim, nodes, data, row };
  });
  return loading;
};

const SCALE = 127;

export const combine = (v: Vectors, terms: Term[]): Float32Array | null => {
  const out = new Float32Array(v.dim);
  const part = new Float32Array(v.dim);
  let any = false;
  for (const t of terms) {
    part.fill(0);
    let members = 0;
    for (const node of t.nodes) {
      const r = v.row[node];
      if (r < 0) continue;
      members++;
      const base = r * v.dim;
      for (let d = 0; d < v.dim; d++) part[d] += v.data[base + d] / SCALE;
    }
    if (members === 0) continue;
    let norm = 0;
    for (let d = 0; d < v.dim; d++) norm += part[d] * part[d];
    norm = Math.sqrt(norm);
    if (norm === 0) continue;
    any = true;
    for (let d = 0; d < v.dim; d++) out[d] += (t.sign * part[d]) / norm;
  }
  if (!any) return null;
  let norm = 0;
  for (let d = 0; d < v.dim; d++) norm += out[d] * out[d];
  norm = Math.sqrt(norm);
  if (norm === 0) return null;
  for (let d = 0; d < v.dim; d++) out[d] /= norm;
  return out;
};

export const dot = (a: Float32Array, b: Float32Array): number => {
  let out = 0;
  for (let d = 0; d < a.length; d++) out += a[d] * b[d];
  return out;
};

export const blend = (parts: [Float32Array, number][]): Float32Array | null => {
  if (parts.length === 0) return null;
  const out = new Float32Array(parts[0][0].length);
  for (const [v, w] of parts) for (let d = 0; d < out.length; d++) out[d] += w * v[d];
  const norm = Math.sqrt(dot(out, out));
  if (norm === 0) return null;
  for (let d = 0; d < out.length; d++) out[d] /= norm;
  return out;
};

export const cosine = (v: Vectors, a: number, b: number): number | null => {
  const ra = v.row[a];
  const rb = v.row[b];
  if (ra < 0 || rb < 0) return null;
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let d = 0; d < v.dim; d++) {
    const x = v.data[ra * v.dim + d];
    const y = v.data[rb * v.dim + d];
    dot += x * y;
    na += x * x;
    nb += y * y;
  }
  return na === 0 || nb === 0 ? 0 : dot / Math.sqrt(na * nb);
};

export const nearest = (v: Vectors, q: Float32Array, take: number, exclude: Set<number>): Hit[] => {
  const hits: Hit[] = [];
  for (let r = 0; r < v.nodes.length; r++) {
    const node = v.nodes[r];
    if (exclude.has(node)) continue;
    const base = r * v.dim;
    let dot = 0;
    for (let d = 0; d < v.dim; d++) dot += q[d] * v.data[base + d];
    const score = dot / SCALE;
    if (hits.length === take && score <= hits[take - 1].score) continue;
    let at = hits.length;
    while (at > 0 && hits[at - 1].score < score) at--;
    hits.splice(at, 0, { node, score });
    if (hits.length > take) hits.pop();
  }
  return hits;
};
