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
    // a typed array view needs its offset aligned to the element size, so copy when it is not
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
