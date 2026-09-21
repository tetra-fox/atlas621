<script lang="ts">
  import type { Communities, Core } from "$lib/data/dataset";
  import type { Names } from "$lib/data/names";
  import type { Hit } from "$lib/data/vectors";
  import { categoryCss, categoryHoverCss } from "$lib/map/palette";
  import { formatCount } from "$lib/search";
  import { tagPath } from "$lib/tagpath";
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";

  type Props = {
    hits: Hit[];
    core: Core;
    names: Names;
    region: Uint16Array;
    communities: Communities;
    onadd?: (name: string, sign: 1 | -1) => void;
  };
  let { hits, core, names, region, communities, onadd }: Props = $props();

  const groups = $derived.by(() => {
    const order: number[] = [];
    const by = new Map<number, Hit[]>();
    for (const hit of hits) {
      const r = region[hit.node];
      let list = by.get(r);
      if (!list) {
        list = [];
        by.set(r, list);
        order.push(r);
      }
      list.push(hit);
    }
    return order.map((r) => ({ region: r, hits: by.get(r) ?? [] }));
  });
  const pct = (x: number) => `${(x * 100).toFixed(0)}%`;
</script>

<div class="flex flex-col gap-2">
  {#each groups as group (group.region)}
    <section>
      <h4 class="mb-0.5 text-xs text-muted">{communities.regions[group.region]?.name}</h4>
      <ol class="flex flex-col gap-1 text-xs">
        {#each group.hits as hit (hit.node)}
          {@const cat = core.categories[hit.node]}
          {@const name = names.at(hit.node)}
          <li
            class="tag-chip"
            style:--tag={categoryCss(cat)}
            style:--tag-alt={categoryHoverCss(cat)}>
            {#if onadd}
              <button
                type="button"
                class="px-1 py-0.5 text-muted hover:text-ink"
                onclick={() => onadd(name, 1)}
                aria-label="add {name}"><Plus class="size-3" /></button>
              <button
                type="button"
                class="border-r border-section-light px-1 py-0.5 text-muted hover:text-ink"
                onclick={() => onadd(name, -1)}
                aria-label="subtract {name}"><Minus class="size-3" /></button>
            {/if}
            <a
              href={tagPath(name)}
              class="min-w-0 flex-1 truncate px-1.5 py-0.5 text-(--tag) hover:text-(--tag-alt)"
              >{name}</a>
            <span class="shrink-0 px-1.5 text-muted tabular-nums"
              >{formatCount(core.postCounts[hit.node])} · {pct(hit.score)}</span>
          </li>
        {/each}
      </ol>
    </section>
  {/each}
</div>
