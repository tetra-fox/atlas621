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

export type Bytes = ReadableStream<Uint8Array<ArrayBuffer>>;

export const begin = (label: string, size: number) => {
  const t = task(label, "bytes");
  t.phase = "downloading";
  t.expected += size;
  report();
};

export const counted = (
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

export const finish = (label: string) => {
  const t = task(label, "bytes");
  t.phase = null;
  t.done = true;
  report();
};
