import { boxHeight } from "$lib/core/atlas";

export const LABEL_PAD = 4;
export const LABEL_GAP_X = 24;
export const LABEL_GAP_Y = 16;
export const LABEL_LEFT = 3;
export const LABEL_SIZES = [10, 11, 12, 13, 14, 15] as const;

export const labelFontSize = (count: number): number =>
  Math.round(Math.min(15, 10 + Math.log10(Math.max(1, count))));
export const labelBoxHeight = (size: number): number => boxHeight(size, LABEL_PAD);
export const LABEL_HALF_MAX = labelBoxHeight(15) / 2;
