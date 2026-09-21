import { buildTable } from "./names";

export type NamesRequest = { count: number; buffer: ArrayBuffer };
export type NamesResponse = { buffer: ArrayBuffer; table: Uint32Array };

self.onmessage = (event: MessageEvent<NamesRequest>) => {
  const { count, buffer } = event.data;
  const offsets = new Uint32Array(buffer, 0, count + 1);
  const bytes = new Uint8Array(buffer, (count + 1) * 4);
  const table = buildTable(offsets, bytes, count);
  const out: NamesResponse = { buffer, table };
  (self as unknown as Worker).postMessage(out, [buffer, table.buffer]);
};
