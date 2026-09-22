import { buildTable } from "$lib/core/strings";
import { readTable, strings } from "$lib/core/table";

export type NamesRequest = { count: number; buffer: ArrayBuffer };
export type NamesResponse = { buffer: ArrayBuffer; table: Uint32Array };

self.onmessage = (event: MessageEvent<NamesRequest>) => {
  const { count, buffer } = event.data;
  const { offsets, bytes } = strings(readTable(buffer), "name");
  const table = buildTable(offsets, bytes, count);
  const out: NamesResponse = { buffer, table };
  (self as unknown as Worker).postMessage(out, [buffer, table.buffer]);
};
