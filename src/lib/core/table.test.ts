import {
  Field,
  FixedSizeList,
  Int8,
  List,
  Table,
  tableFromArrays,
  tableToIPC,
  Uint32,
  Utf8,
  vectorFromArray,
  type Vector
} from "apache-arrow";
import { describe, expect, it } from "vitest";

import { readTable, runs, strings, values } from "./table";

// everything goes through a real ipc round trip, since that is the only way these columns ever
// arrive and the decoded buffers are what the callers index into
const roundTrip = (columns: Record<string, Vector>) =>
  readTable(tableToIPC(new Table(columns)).buffer as ArrayBuffer);

const list = (rows: number[][]) => vectorFromArray(rows, new List(new Field("item", new Uint32())));
const fixed = (rows: number[][], width: number) =>
  vectorFromArray(rows, new FixedSizeList(width, new Field("item", new Int8())));

describe("values", () => {
  it("returns the backing store of a primitive column", () => {
    const t = readTable(
      tableToIPC(tableFromArrays({ rank: new Uint32Array([7, 8, 9]) })).buffer as ArrayBuffer
    );
    expect(Array.from(values<Uint32Array>(t, "rank"))).toEqual([7, 8, 9]);
  });

  // vectors.bin.gz ships one fixed width row per node; callers index the flat elements by
  // row * dim, so values has to reach through the list to its child
  it("flattens a fixed width list to its elements", () => {
    const t = roundTrip({
      vector: fixed(
        [
          [1, 2, 3],
          [4, 5, 6]
        ],
        3
      )
    });
    expect(Array.from(values<Int8Array>(t, "vector"))).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it("names the columns it does have when one is missing", () => {
    const t = readTable(
      tableToIPC(tableFromArrays({ rank: new Uint32Array([1]) })).buffer as ArrayBuffer
    );
    expect(() => values(t, "weight")).toThrow(/no weight column/);
    expect(() => values(t, "weight")).toThrow(/rank/);
  });
});

describe("runs", () => {
  it("returns offsets alongside the flat elements", () => {
    const t = roundTrip({ far: list([[1, 2], [3], [], [4, 5, 6]]) });
    const { off, values: flat } = runs<Uint32Array>(t, "far");
    expect(Array.from(off).slice(0, 5)).toEqual([0, 2, 3, 3, 6]);
    expect(Array.from(flat)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  // an empty row is the case where an off-by-one in the offsets would go unnoticed, because
  // the flat elements still line up
  it("keeps an empty row as a zero width span", () => {
    const t = roundTrip({ far: list([[1], [], [2]]) });
    const { off, values: flat } = runs<Uint32Array>(t, "far");
    const row = (i: number) => Array.from(flat.subarray(off[i], off[i + 1]));
    expect(row(0)).toEqual([1]);
    expect(row(1)).toEqual([]);
    expect(row(2)).toEqual([2]);
  });

  it("rejects a column that is not a list", () => {
    const t = readTable(
      tableToIPC(tableFromArrays({ rank: new Uint32Array([1]) })).buffer as ArrayBuffer
    );
    expect(() => runs(t, "rank")).toThrow(/not a list column/);
  });
});

describe("strings", () => {
  it("returns offsets and utf-8 bytes that slice back to the originals", () => {
    const words = ["fox", "ñandu", "🦊", ""];
    const t = roundTrip({ name: vectorFromArray(words, new Utf8()) });
    const { offsets, bytes } = strings(t, "name");
    const decoder = new TextDecoder();
    const read = (i: number) => decoder.decode(bytes.subarray(offsets[i], offsets[i + 1]));
    expect(words.map((_, i) => read(i))).toEqual(words);
  });

  it("measures multi-byte names in bytes, not characters", () => {
    const t = roundTrip({ name: vectorFromArray(["🦊"], new Utf8()) });
    const { offsets } = strings(t, "name");
    expect(offsets[1] - offsets[0]).toBe(4);
  });

  it("rejects a column that is not a string column", () => {
    const t = readTable(
      tableToIPC(tableFromArrays({ rank: new Uint32Array([1]) })).buffer as ArrayBuffer
    );
    expect(() => strings(t, "rank")).toThrow(/not a string column/);
  });
});
