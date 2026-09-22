const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

const hashBytes = (bytes: Uint8Array, start: number, end: number): number => {
  let h = FNV_OFFSET;
  for (let i = start; i < end; i++) {
    h ^= bytes[i];
    h = Math.imul(h, FNV_PRIME);
  }
  return h >>> 0;
};

const tableSize = (count: number): number => 1 << Math.ceil(Math.log2(Math.max(2, count * 2)));

export const buildTable = (offsets: Int32Array, bytes: Uint8Array, count: number): Uint32Array => {
  const table = new Uint32Array(tableSize(count));
  const mask = table.length - 1;
  for (let i = 0; i < count; i++) {
    let slot = hashBytes(bytes, offsets[i], offsets[i + 1]) & mask;
    while (table[slot] !== 0) slot = (slot + 1) & mask;
    table[slot] = i + 1;
  }
  return table;
};

export class Names {
  readonly length: number;
  private readonly decoded = new Map<number, string>();
  private readonly decoder = new TextDecoder();
  private readonly encoder = new TextEncoder();

  constructor(
    readonly offsets: Int32Array,
    readonly bytes: Uint8Array,
    private readonly table: Uint32Array
  ) {
    this.length = offsets.length - 1;
  }

  at(i: number): string {
    let s = this.decoded.get(i);
    if (s === undefined) {
      s = this.decoder.decode(this.bytes.subarray(this.offsets[i], this.offsets[i + 1]));
      this.decoded.set(i, s);
    }
    return s;
  }

  indexOf(name: string): number | undefined {
    const q = this.encoder.encode(name);
    const mask = this.table.length - 1;
    let slot = hashBytes(q, 0, q.length) & mask;
    for (;;) {
      const v = this.table[slot];
      if (v === 0) return undefined;
      const i = v - 1;
      const start = this.offsets[i];
      if (this.offsets[i + 1] - start === q.length) {
        let k = 0;
        while (k < q.length && this.bytes[start + k] === q[k]) k++;
        if (k === q.length) return i;
      }
      slot = (slot + 1) & mask;
    }
  }
}
