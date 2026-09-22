<script lang="ts">
  import type { Names } from "$lib/core/strings";
  import type { Core, SearchEntry } from "$lib/data/dataset";
  import { formatCount, search, topByCount, type SearchResult } from "$lib/data/search";
  import { CATEGORY_NAMES, categoryCss } from "$lib/render/palette";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Search from "@lucide/svelte/icons/search";
  import { Combobox } from "bits-ui";

  type Props = { core: Core | null; names: Names | null; onpick: (entry: SearchEntry) => void };
  let { core, names, onpick }: Props = $props();

  const topTags = $derived(
    core && names ? topByCount(names, core.postCounts, core.categories) : []
  );

  let results = $state.raw<SearchResult[]>([]);
  let open = $state(false);
  let request = 0;
  let input = $state<HTMLInputElement | null>(null);
  export const focus = () => input?.focus();

  const run = async (value: string) => {
    const id = ++request;
    const found = value.trim().length === 0 ? topTags : await search(value);
    if (id !== request) return;
    results = found;
    open = found.length > 0 || value.trim().length > 0;
  };

  const keyOf = (r: SearchResult) => `${r.name}\n${r.alias ?? ""}`;

  let picked = $state("");
  const onValueChange = (value: string) => {
    const hit = results.find((r) => keyOf(r) === value);
    if (!hit) return;
    picked = "";
    onpick(hit);
  };
</script>

<Combobox.Root type="single" bind:value={picked} bind:open {onValueChange}>
  <div class="flex">
    <Combobox.Input
      bind:ref={input}
      placeholder="find a tag"
      autocomplete="off"
      spellcheck="false"
      oninput={(e) => run(e.currentTarget.value)}
      onfocus={(e) => run(e.currentTarget.value)}
      class="min-w-0 flex-1 rounded-r-none" />
    <Combobox.Trigger
      class="flex w-8 shrink-0 items-center justify-center rounded-r-xs border-l border-black/20 bg-field text-field-ink hover:bg-field-focus active:bg-field-focus"
      aria-label="show matches">
      <Search class="size-4" strokeWidth={2.5} />
    </Combobox.Trigger>
  </div>
  <Combobox.Portal>
    <Combobox.Content
      sideOffset={2}
      class="z-20 max-h-96 w-(--bits-floating-anchor-width) overflow-y-auto rounded-sm border border-section-dark bg-section-light py-0.5 shadow-drop">
      {#if results.length === 0}
        <p class="px-2 py-0.5 text-xs text-muted">no tags match</p>
      {/if}
      {#each results as r (keyOf(r))}
        <Combobox.Item
          value={keyOf(r)}
          label={r.alias ?? r.name}
          class="flex cursor-pointer items-baseline gap-2 px-2 py-0.5 text-sm active:bg-ground data-highlighted:bg-section-dark">
          <span class="truncate" style:color={categoryCss(r.category)}>{r.name}</span>
          {#if r.alias}
            <ArrowRight class="size-3 shrink-0 self-center text-muted" aria-hidden="true" />
            <span class="truncate text-xs text-muted">{r.alias}</span>
          {/if}
          <span class="ml-auto shrink-0 text-xs text-muted">
            {formatCount(r.postCount)}{r.node < 0 ? " · off map" : ""}
          </span>
          <span class="sr-only">{CATEGORY_NAMES[r.category]}</span>
        </Combobox.Item>
      {/each}
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
