import { resolveQuery, type Resolved } from "$lib/data/query";
import type { Term } from "$lib/data/vectors";

export class Slot {
  text = $state("");
  resolved = $state.raw<Resolved | null>(null);
  #request = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(initial: string) {
    this.text = initial;
    $effect(() => {
      const text = this.text;
      const id = ++this.#request;
      clearTimeout(this.#timer);
      this.#timer = setTimeout(async () => {
        const out = await resolveQuery(text);
        if (id === this.#request) this.resolved = out;
      }, 150);
    });
  }

  get terms(): Term[] {
    return this.resolved?.terms.map((t) => ({ sign: t.sign, nodes: t.tags.map((e) => e.i) })) ?? [];
  }

  get nodes(): number[] {
    return this.terms.flatMap((t) => t.nodes);
  }
}
