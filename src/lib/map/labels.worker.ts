import { labelZooms, type LabelZooms } from "$lib/core/declutter";
import {
  LABEL_GAP_X,
  LABEL_GAP_Y,
  LABEL_PAD,
  LABEL_SIZES,
  labelBoxHeight,
  labelFontSize
} from "$lib/render/labels";
import { radiusFor } from "$lib/render/palette";

export type LabelsRequest = {
  positions: Float32Array;
  postCounts: Uint32Array;
  offsets: Int32Array;
  bytes: Uint8Array;
  font: string;
  space: number;
  zoomMax: number;
};
export type LabelsResponse = LabelZooms & { done: number };

const labelWidths = (
  offsets: Int32Array,
  bytes: Uint8Array,
  postCounts: Uint32Array,
  font: string
): Float32Array => {
  const ctx = new OffscreenCanvas(1, 1).getContext("2d");
  if (!ctx) throw new Error("labels: no 2d context");
  const fontOf = (size: number) => `500 ${size}px ${font}`;
  const advances = LABEL_SIZES.map((size) => {
    ctx.font = fontOf(size);
    const table = new Float32Array(128);
    for (let c = 32; c < 127; c++) table[c] = ctx.measureText(String.fromCharCode(c)).width;
    return table;
  });
  const decoder = new TextDecoder();
  const n = postCounts.length;
  const widths = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const size = labelFontSize(postCounts[i]);
    const table = advances[size - LABEL_SIZES[0]];
    const start = offsets[i];
    const end = offsets[i + 1];
    let width = 0;
    let k = start;
    for (; k < end; k++) {
      const c = bytes[k];
      if (c < 32 || c > 126) break;
      width += table[c];
    }
    if (k < end) {
      ctx.font = fontOf(size);
      width = ctx.measureText(decoder.decode(bytes.subarray(start, end))).width;
    }
    widths[i] = Math.ceil(width) + LABEL_PAD * 2;
  }
  return widths;
};

const post = (zooms: LabelZooms, done: number) => {
  const out: LabelsResponse = { ...zooms, done };
  (self as unknown as Worker).postMessage(out, [
    zooms.zoom.buffer,
    zooms.order.buffer,
    zooms.starts.buffer
  ]);
};

self.onmessage = (event: MessageEvent<LabelsRequest>) => {
  const { positions, postCounts, offsets, bytes, font, space, zoomMax } = event.data;
  const widths = labelWidths(offsets, bytes, postCounts, font);
  const n = postCounts.length;
  const reach = new Float32Array(n);
  const halfH = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    reach[i] = 2 * radiusFor(postCounts[i]) + widths[i] + LABEL_GAP_X;
    halfH[i] = labelBoxHeight(labelFontSize(postCounts[i])) / 2;
  }
  post(labelZooms(positions, reach, halfH, LABEL_GAP_Y, space, zoomMax, post), n);
};
