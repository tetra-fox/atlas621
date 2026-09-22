<script lang="ts">
  import type { Names } from "$lib/core/strings";
  import {
    loadNodeText,
    type Communities,
    type Core,
    type NodeText,
    type SearchEntry,
    type Years
  } from "$lib/data/dataset";
  import { WEIGHT_MAX } from "$lib/data/manifest";
  import { findPath } from "$lib/data/paths";
  import { compactCount, formatCount } from "$lib/data/search";
  import { CATEGORY_NAMES, categoryCss, categoryHoverCss } from "$lib/render/palette";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import X from "@lucide/svelte/icons/x";
  import NumberFlow from "@number-flow/svelte";
  import { Collapsible } from "bits-ui";
  import { tick } from "svelte";

  import { yearRow } from "./graphdata";
  import SearchBox from "./SearchBox.svelte";
  import Sparkline from "./Sparkline.svelte";
  import { getMapState } from "./state.svelte";

  type Props = {
    node: number;
    core: Core;
    names: Names;
    years: Years | null;
    firstYear: Uint16Array | null;
    communities: Communities;
    onnavigate: (node: number) => void;
  };
  let { node, core, names, years, firstYear, communities, onnavigate }: Props = $props();
  const ui = getMapState();

  const name = $derived(names.at(node));
  const category = $derived(core.categories[node]);
  const count = $derived(compactCount(core.postCounts[node]));
  const span = $derived(core.manifest.years.length);
  const yearly = $derived(years ? yearRow(years, span, node) : null);
  let text = $state.raw<{ node: number; t: NodeText | undefined; failed: boolean } | null>(null);
  const split = $derived(text?.t?.r ?? null);
  const regionName = $derived(
    text?.t?.g === undefined ? null : (communities.regions[text.t.g]?.name ?? null)
  );
  const neighbors = $derived.by(() => {
    const e = text?.t?.e;
    if (!e) return [];
    const out: { node: number; weight: number; count: number }[] = [];
    for (let k = 0; k + 2 < e.length; k += 3)
      out.push({ node: e[k], weight: e[k + 1] / WEIGHT_MAX, count: e[k + 2] });
    return out;
  });
  const similar = $derived.by(() => {
    const s = text?.t?.s;
    if (!s) return [];
    const out: { node: number; weight: number }[] = [];
    for (let k = 0; k + 1 < s.length; k += 2)
      out.push({ node: s[k], weight: s[k + 1] / WEIGHT_MAX });
    return out;
  });
  $effect(() => {
    const n = node;
    let live = true;
    loadNodeText(core.manifest, n).then(
      (t) => {
        if (live) text = { node: n, t, failed: false };
      },
      () => {
        if (live) text = { node: n, t: undefined, failed: true };
      }
    );
    return () => {
      live = false;
    };
  });
  const stale = $derived(text !== null && text.node !== node);

  const e621Search = $derived(`https://e621.net/posts?tags=${encodeURIComponent(name)}`);
  const wikiUrl = (tag: string) =>
    `https://e621.net/wiki_pages/show_or_new?title=${encodeURIComponent(tag)}`;

  const jump = (tag: string) => {
    const idx = names.indexOf(tag);
    if (idx !== undefined) onnavigate(idx);
  };
  const categoryOf = (tag: string): number | null => {
    const idx = names.indexOf(tag);
    return idx === undefined ? null : core.categories[idx];
  };

  let historyOpen = $state(false);
  let impliedAll = $state(false);

  let pathPicking = $state(false);
  let pathBox = $state<ReturnType<typeof SearchBox>>();
  const togglePath = async () => {
    pathPicking = !pathPicking;
    if (!pathPicking) return;
    await tick();
    pathBox?.focus();
  };
  let pathBusy = $state(false);
  let pathMissing = $state(false);
  const pathTo = async (entry: SearchEntry) => {
    if (entry.i < 0) return;
    pathPicking = false;
    pathBusy = true;
    pathMissing = false;
    try {
      const found = await findPath(core.manifest, node, entry.i);
      ui.path = found;
      pathMissing = found === null;
    } finally {
      pathBusy = false;
    }
  };
</script>

{#snippet neighborChip(other: number, note: string)}
  {@const k = core.categories[other]}
  <li class="tag-chip" style:--tag={categoryCss(k)} style:--tag-alt={categoryHoverCss(k)}>
    <a
      href={wikiUrl(names.at(other))}
      target="_blank"
      rel="noopener"
      class="px-1.5 py-0.5"
      title="wiki">?</a>
    <button
      type="button"
      class="min-w-0 flex-1 truncate border-l border-section-light px-1.5 py-0.5 text-left text-(--tag) hover:text-(--tag-alt)"
      onclick={() => onnavigate(other)}>
      {names.at(other)}
    </button>
    <span class="shrink-0 px-1.5 text-muted tabular-nums">{note}</span>
  </li>
{/snippet}

{#snippet tagLink(tag: string)}
  {@const k = categoryOf(tag)}
  <button
    type="button"
    class={[
      "ml-1",
      k === null ? "text-link hover:text-link-hover" : "text-(--tag) hover:text-(--tag-alt)"
    ]}
    style:--tag={k === null ? undefined : categoryCss(k)}
    style:--tag-alt={k === null ? undefined : categoryHoverCss(k)}
    onclick={() => jump(tag)}>{tag}</button>
{/snippet}

<aside
  class="flex min-h-0 w-96 max-w-[calc(100vw-1rem)] flex-col gap-3 overflow-y-auto rounded-sm bg-page/95 p-3 text-sm backdrop-blur">
  {#if ui.trail.length}
    <nav aria-label="trail" class="flex flex-wrap items-center gap-x-1 text-xs">
      {#each ui.trail as crumb (crumb)}
        {@const k = core.categories[crumb]}
        <button
          type="button"
          class="max-w-40 truncate text-(--tag) hover:text-(--tag-alt)"
          style:--tag={categoryCss(k)}
          style:--tag-alt={categoryHoverCss(k)}
          title={names.at(crumb)}
          onclick={() => onnavigate(crumb)}>{names.at(crumb)}</button>
        <span class="text-muted">›</span>
      {/each}
      <span class="max-w-40 truncate text-muted" aria-current="page" title={name}>{name}</span>
    </nav>
  {/if}
  <header class="flex items-start justify-between gap-2">
    <div class="min-w-0">
      <h2 class="truncate text-lg font-bold" style:color={categoryCss(category)} title={name}>
        {name}
      </h2>
      <p class="text-xs text-muted">
        {CATEGORY_NAMES[category]} ·
        <NumberFlow
          value={count.value}
          format={{ minimumFractionDigits: count.fraction, maximumFractionDigits: count.fraction }}
          suffix={count.suffix} /> posts
        {#if firstYear}· since {firstYear[node]}{/if}
      </p>
    </div>
    <button
      type="button"
      class="btn px-1.5 py-0.5"
      onclick={() => (ui.selected = null)}
      aria-label="close"><X class="size-3.5" /></button>
  </header>

  <p class="flex gap-1 text-xs">
    <a href={e621Search} target="_blank" rel="noopener" class="btn"
      >posts on e621 <ExternalLink class="size-3" aria-hidden="true" /></a>
    <a href={wikiUrl(name)} target="_blank" rel="noopener" class="btn"
      >wiki <ExternalLink class="size-3" aria-hidden="true" /></a>
    <button
      type="button"
      class={["btn", pathPicking && "bg-field-focus"]}
      aria-pressed={pathPicking}
      onclick={togglePath}>path to</button>
  </p>
  {#if pathPicking}
    <SearchBox bind:this={pathBox} {core} {names} onpick={pathTo} />
  {/if}
  {#if pathBusy}
    <p class="text-xs text-muted">finding a path</p>
  {:else if pathMissing}
    <p class="text-xs text-muted">no chain of shared posts connects them</p>
  {/if}
  {#if ui.path}
    <section>
      <h3 class="mb-1 flex items-baseline justify-between text-base font-bold">
        path
        <button
          type="button"
          class="text-xs font-normal text-muted hover:text-link-hover"
          onclick={() => (ui.path = null)}>clear</button>
      </h3>
      <ul class="flex flex-col gap-1 text-xs">
        {#each ui.path.nodes as n, k (n)}
          {@render neighborChip(n, k > 0 ? `${(ui.path.weights[k - 1] * 100).toFixed(0)}%` : "")}
        {/each}
      </ul>
    </section>
  {/if}

  {#if yearly}
    <section>
      <Sparkline values={yearly} years={core.manifest.years} highlight={ui.yearRange} />
    </section>
  {/if}

  {#if text === null}
    <p class="text-xs text-muted">loading</p>
  {:else if text.failed}
    <p class="text-xs text-muted">no details available</p>
  {:else}
    {@const t = text.t}
    <div class={["flex flex-col gap-3 transition-opacity", stale && "opacity-60"]}>
      {#if split}
        {@const total = split[0] + split[1] + split[2] || 1}
        {@const pct = split.map((v) => Math.round((v / total) * 100))}
        <section>
          <div
            class="flex h-1.5 overflow-hidden rounded-xs bg-section"
            role="img"
            aria-label="safe {pct[0]}%, questionable {pct[1]}%, explicit {pct[2]}%">
            <div class="bg-safe-fill" style:width="{(split[0] / total) * 100}%"></div>
            <div class="bg-questionable-fill" style:width="{(split[1] / total) * 100}%"></div>
            <div class="bg-explicit-fill" style:width="{(split[2] / total) * 100}%"></div>
          </div>
          <p class="mt-1 flex gap-3 text-xs font-bold">
            <span class="text-safe">safe {pct[0]}%</span>
            <span class="text-questionable">questionable {pct[1]}%</span>
            <span class="text-explicit">explicit {pct[2]}%</span>
          </p>
        </section>
      {/if}
      {#if regionName}
        <p class="text-xs text-muted">island: {regionName}</p>
      {/if}
      {#if t?.w}
        <p class="rounded-sm bg-section p-2 text-xs">{t.w}</p>
      {/if}
      {#if t?.p?.length || t?.c?.length}
        <section class="text-xs">
          {#if t.p?.length}
            <p>
              <span class="text-muted">implies</span>
              {#each t.p as tag (tag)}{@render tagLink(tag)}{/each}
            </p>
          {/if}
          {#if t.c?.length}
            <p class="mt-1">
              <span class="text-muted">implied by ({t.c.length})</span>
              {#each impliedAll ? t.c : t.c.slice(0, 30) as tag (tag)}{@render tagLink(tag)}{/each}
              {#if t.c.length > 30}
                <button
                  type="button"
                  class="ml-1 text-muted hover:text-link-hover"
                  onclick={() => (impliedAll = !impliedAll)}
                  >{impliedAll ? "fewer" : `and ${t.c.length - 30} more`}</button>
              {/if}
            </p>
          {/if}
        </section>
      {/if}
      {#if t?.a?.length}
        <p class="text-xs"><span class="text-muted">also known as</span> {t.a.join(", ")}</p>
      {/if}
      {#if t?.l?.length}
        <p class="text-xs">
          <span class="text-muted">see also</span>
          {#each t.l.slice(0, 20) as idx (idx)}{@render tagLink(names.at(idx))}{/each}
        </p>
      {/if}
      {#if t?.h?.length}
        <Collapsible.Root bind:open={historyOpen} class="text-xs">
          <Collapsible.Trigger class="font-bold hover:text-link-hover">
            <span
              class={["inline-block align-[-2px] transition-transform", historyOpen && "rotate-90"]}
              ><ChevronRight class="size-3.5" /></span>
            history ({t.h.length})
          </Collapsible.Trigger>
          <Collapsible.Content>
            <ul class="mt-1 space-y-0.5">
              {#each t.h.slice().reverse() as ev, i (i)}
                <li class="flex gap-2">
                  <span class="text-muted tabular-nums">{ev.d}</span>
                  <span>{ev.k}</span>
                  <span class="truncate">{@render tagLink(ev.o)}</span>
                  {#if ev.a}<span class="text-muted" title="date comes from a bulk migration"
                      >~</span
                    >{/if}
                </li>
              {/each}
            </ul>
          </Collapsible.Content>
        </Collapsible.Root>
      {/if}
      {#if neighbors.length}
        <section>
          <h3 class="mb-1 text-base font-bold">appears with</h3>
          <ul class="flex flex-col gap-1 text-xs">
            {#each neighbors as nb (nb.node)}
              {@render neighborChip(
                nb.node,
                `${formatCount(nb.count)} · ${(nb.weight * 100).toFixed(0)}%`
              )}
            {/each}
          </ul>
        </section>
      {/if}
      {#if similar.length}
        <section>
          <h3 class="mb-1 text-base font-bold">similar tags</h3>
          <ul class="flex flex-col gap-1 text-xs">
            {#each similar as nb (nb.node)}
              {@render neighborChip(nb.node, `${(nb.weight * 100).toFixed(0)}%`)}
            {/each}
          </ul>
        </section>
      {/if}
    </div>
  {/if}
</aside>
