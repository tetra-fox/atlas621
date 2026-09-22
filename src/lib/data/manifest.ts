// link weights ship as a u16 fraction of this. the pipeline states it in the manifest rather
// than both sides assuming; the initial value is only what stands before one is loaded
export let WEIGHT_MAX = 65535;
export const setWeightMax = (max: number) => {
  WEIGHT_MAX = max;
};

export type { Manifest } from "./generated";
