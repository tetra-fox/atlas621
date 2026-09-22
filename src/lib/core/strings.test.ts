import { describe, expect, it } from "vitest";

import { buildTable, Names } from "./strings";

const pack = (words: string[]): Names => {
  const encoder = new TextEncoder();
  const parts = words.map((w) => encoder.encode(w));
  const offsets = new Int32Array(parts.length + 1);
  for (let i = 0; i < parts.length; i++) offsets[i + 1] = offsets[i] + parts[i].length;
  const bytes = new Uint8Array(offsets[parts.length]);
  for (let i = 0; i < parts.length; i++) bytes.set(parts[i], offsets[i]);
  return new Names(offsets, bytes, buildTable(offsets, bytes, parts.length));
};

describe("Names", () => {
  const words = ["fox", "red_fox", "canine", "wolf", "dragon", "anthro"];

  it("reads every name back", () => {
    const names = pack(words);
    expect(names.length).toBe(words.length);
    expect(words.map((_, i) => names.at(i))).toEqual(words);
  });

  it("finds every name it holds", () => {
    const names = pack(words);
    for (const [i, w] of words.entries()) expect(names.indexOf(w)).toBe(i);
  });

  it("reports a miss rather than a neighbouring slot", () => {
    const names = pack(words);
    expect(names.indexOf("cat")).toBeUndefined();
    expect(names.indexOf("")).toBeUndefined();
  });

  // a prefix hashes differently but open addressing can still walk it onto the longer name's
  // slot, so the length check in indexOf is what keeps them apart
  it("does not confuse a name with a prefix or extension of it", () => {
    const names = pack(["fox", "foxes", "fo", "f"]);
    expect(names.indexOf("fox")).toBe(0);
    expect(names.indexOf("foxes")).toBe(1);
    expect(names.indexOf("fo")).toBe(2);
    expect(names.indexOf("f")).toBe(3);
    expect(names.indexOf("foxe")).toBeUndefined();
  });

  it("round-trips multi-byte utf-8", () => {
    const names = pack(["ñandu", "日本", "🦊"]);
    expect(names.at(0)).toBe("ñandu");
    expect(names.at(2)).toBe("🦊");
    expect(names.indexOf("日本")).toBe(1);
    expect(names.indexOf("🦊")).toBe(2);
  });

  it("handles empty names without colliding them with a miss", () => {
    const names = pack(["", "a"]);
    expect(names.at(0)).toBe("");
    expect(names.indexOf("a")).toBe(1);
  });

  // the table is power-of-two sized and probes linearly, so a load factor near one is where
  // wraparound and full-table scans would show up
  it("survives a table packed to its resize boundary", () => {
    const many = Array.from({ length: 512 }, (_, i) => `tag_${i}`);
    const names = pack(many);
    for (const [i, w] of many.entries()) expect(names.indexOf(w)).toBe(i);
    expect(names.indexOf("tag_512")).toBeUndefined();
  });
});
