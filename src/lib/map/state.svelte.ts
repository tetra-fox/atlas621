import type { TagPath } from "$lib/data/paths";
import { CATEGORY_COUNT } from "$lib/render/palette";
import { createContext } from "svelte";

import { FULL_DENSITY } from "./linkselect";

export type ColorMode = "category" | "birth";

export class MapState {
  #selected = $state<number | null>(null);
  trail = $state.raw<number[]>([]);
  path = $state.raw<TagPath | null>(null);
  hovered = $state<number | null>(null);
  yearRange = $state<[number, number] | null>(null);
  colorMode = $state<ColorMode>("category");
  edgeDensity = $state(FULL_DENSITY);
  categoriesOn = $state<boolean[]>(Array.from({ length: CATEGORY_COUNT }, () => true));
  showImplications = $state(false);
  territories = $state(true);
  labels = $state(true);

  categoryMask = $derived(this.categoriesOn.reduce((m, on, k) => (on ? m | (1 << k) : m), 0));

  get selected() {
    return this.#selected;
  }
  set selected(node: number | null) {
    if (node === this.#selected) return;
    this.trail = [];
    if (node === null) this.path = null;
    this.#selected = node;
  }

  follow(node: number) {
    const prev = this.#selected;
    if (node === prev) return;
    const at = this.trail.indexOf(node);
    if (at >= 0) this.trail = this.trail.slice(0, at);
    else if (prev !== null) this.trail = [...this.trail, prev];
    this.#selected = node;
  }
}

export const [getMapState, setMapState] = createContext<MapState>();
