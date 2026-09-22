import type { Manifest } from "./manifest";
import { begin, counted, finish, type Bytes } from "./progress";

const DATA_BASE = "/data";

let version = "";
export const setDataVersion = (generatedAt: string) => {
  version = `?v=${encodeURIComponent(generatedAt)}`;
};
export const dataUrl = (path: string) => `${DATA_BASE}/${path}${version}`;

const peek = async (stream: Bytes, n: number): Promise<[Uint8Array, Bytes]> => {
  const reader = stream.getReader();
  const taken: Uint8Array<ArrayBuffer>[] = [];
  let have = 0;
  while (have < n) {
    const { done, value } = await reader.read();
    if (done) break;
    taken.push(value);
    have += value.byteLength;
  }
  const head = new Uint8Array(Math.min(n, have));
  let at = 0;
  for (const chunk of taken) {
    const take = Math.min(chunk.byteLength, head.length - at);
    head.set(chunk.subarray(0, take), at);
    at += take;
  }
  const again: Bytes = new ReadableStream({
    start(controller) {
      for (const chunk of taken) controller.enqueue(chunk);
      if (have < n) controller.close();
    },
    async pull(controller) {
      const { done, value } = await reader.read();
      if (done) controller.close();
      else controller.enqueue(value);
    }
  });
  return [head, again];
};

const body = (path: string, res: Response): Bytes => {
  if (!res.ok) throw new Error(`${path}: ${res.status}`);
  if (!res.body) throw new Error(`${path}: no body`);
  return res.body;
};

export const fetchJson = async <T>(path: string, size?: number, label?: string): Promise<T> => {
  const counting = size !== undefined && label !== undefined;
  if (counting) begin(label, size);
  const res = await fetch(dataUrl(path));
  const raw = body(path, res);
  const value = (await new Response(
    counting ? counted(raw, size, label, "parsing") : raw
  ).json()) as T;
  if (counting) finish(label);
  return value;
};

export const fetchGz = async (
  path: string,
  size?: number,
  label?: string
): Promise<ArrayBuffer> => {
  const counting = size !== undefined && label !== undefined;
  if (counting) begin(label, size);
  const res = await fetch(dataUrl(path));
  const raw = body(path, res);
  const stream = counting ? counted(raw, size, label, "inflating") : raw;
  // in dev the file arrives already decoded through content-encoding, so sniff the gzip
  // magic rather than assuming the body is still compressed
  const [head, rest] = await peek(stream, 2);
  const gz = head.length === 2 && head[0] === 0x1f && head[1] === 0x8b;
  const buffer = await new Response(
    gz ? rest.pipeThrough(new DecompressionStream("gzip")) : rest
  ).arrayBuffer();
  if (label !== undefined) finish(label);
  return buffer;
};

export const fetchArray = async (
  manifest: Manifest,
  name: string,
  label?: string
): Promise<ArrayBuffer> => {
  const count = manifest.parts[name];
  if (!count) return fetchGz(name, manifest.files[name], label);
  const stem = name.replace(/\.bin\.gz$/, "");
  const parts = await Promise.all(
    Array.from({ length: count }, (_, k) => {
      const part = `${stem}.${k}.bin.gz`;
      return fetchGz(part, manifest.files[part], label && `${label} ${k + 1}/${count}`);
    })
  );
  const total = parts.reduce((sum, p) => sum + p.byteLength, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const p of parts) {
    out.set(new Uint8Array(p), offset);
    offset += p.byteLength;
  }
  return out.buffer;
};
