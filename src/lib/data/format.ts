const DATA_BASE = "/data";

let version = "";
export const setDataVersion = (generatedAt: string) => {
  version = `?v=${encodeURIComponent(generatedAt)}`;
};
export const dataUrl = (path: string) => `${DATA_BASE}/${path}${version}`;

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

export type Task = {
  label: string;
  kind: "bytes" | "step";
  phase: "downloading" | "inflating" | "parsing" | null;
  loaded: number;
  expected: number;
  done: boolean;
};
export type Progress = { tasks: Task[] };
export const STEPS = [
  "fetching the file list",
  "indexing tags",
  "placing tags",
  "choosing edges",
  "drawing edges",
  "compiling shaders"
] as const;
export type Step = (typeof STEPS)[number];

const tasks: Task[] = [];
let onProgress: ((p: Progress) => void) | null = null;
export const watchProgress = (fn: typeof onProgress) => {
  onProgress = fn;
};
const report = () => onProgress?.({ tasks: tasks.map((t) => ({ ...t })) });
const task = (label: string, kind: Task["kind"]): Task => {
  let t = tasks.find((t) => t.label === label);
  if (!t) {
    t = { label, kind, phase: null, loaded: 0, expected: 0, done: false };
    tasks.push(t);
  }
  return t;
};
export const beginStep = (step: Step) => {
  task(step, "step").done = false;
  report();
};
export const endStep = (step: Step) => {
  task(step, "step").done = true;
  report();
};
export const resetProgress = () => {
  tasks.length = 0;
  report();
};
export const paint = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));

type Bytes = ReadableStream<Uint8Array<ArrayBuffer>>;

const begin = (label: string, size: number) => {
  const t = task(label, "bytes");
  t.phase = "downloading";
  t.expected += size;
  report();
};

const counted = (
  body: Bytes,
  size: number,
  label: string,
  then: "inflating" | "parsing"
): Bytes => {
  const t = task(label, "bytes");
  let got = 0;
  let credited = 0;
  const credit = (upTo: number) => {
    const share = Math.min(size, upTo) - credited;
    credited += share;
    t.loaded += share;
    report();
  };
  return body.pipeThrough(
    new TransformStream<Uint8Array<ArrayBuffer>, Uint8Array<ArrayBuffer>>({
      transform(chunk, controller) {
        got += chunk.byteLength;
        credit(got);
        controller.enqueue(chunk);
      },
      flush() {
        credit(size);
        t.phase = then;
        report();
      }
    })
  );
};

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

const finish = (label: string) => {
  const t = task(label, "bytes");
  t.phase = null;
  t.done = true;
  report();
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
  const [head, rest] = await peek(stream, 2);
  const gz = head.length === 2 && head[0] === 0x1f && head[1] === 0x8b;
  const buffer = await new Response(
    gz ? rest.pipeThrough(new DecompressionStream("gzip")) : rest
  ).arrayBuffer();
  if (label !== undefined) finish(label);
  return buffer;
};

export class Cursor {
  private offset = 0;
  constructor(private readonly buffer: ArrayBuffer) {}

  private take<T>(
    ctor: { new (b: ArrayBuffer, o: number, n: number): T; BYTES_PER_ELEMENT: number },
    length: number
  ): T {
    const bytes = length * ctor.BYTES_PER_ELEMENT;
    if (this.offset + bytes > this.buffer.byteLength) {
      throw new Error(
        `file too short: wanted ${bytes} bytes at ${this.offset}, have ${this.buffer.byteLength}`
      );
    }
    const view =
      this.offset % ctor.BYTES_PER_ELEMENT === 0
        ? new ctor(this.buffer, this.offset, length)
        : new ctor(this.buffer.slice(this.offset, this.offset + bytes), 0, length);
    this.offset += bytes;
    return view;
  }

  u8 = (n: number): Uint8Array => this.take(Uint8Array, n);
  u16 = (n: number): Uint16Array => this.take(Uint16Array, n);
  u32 = (n: number): Uint32Array => this.take(Uint32Array, n);
  f32 = (n: number): Float32Array => this.take(Float32Array, n);

  get remaining(): number {
    return this.buffer.byteLength - this.offset;
  }
}

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
