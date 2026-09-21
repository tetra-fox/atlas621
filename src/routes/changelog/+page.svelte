<script lang="ts">
  import { replaceState } from "$app/navigation";
  import { resolve } from "$app/paths";
  import { page } from "$app/state";
  import { loadChangelog, type ChangelogEntry } from "$lib/data/dataset";
  import Nav from "$lib/Nav.svelte";
  import { tagPath } from "$lib/tagpath";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { Select } from "bits-ui";
  import { SvelteSet } from "svelte/reactivity";

  const initial = page.url.searchParams;
  const PAGE_SIZES = [25, 50, 100, 250];
  const DEFAULT_PER_PAGE = 50;
  const initialPer = Number(initial.get("per"));

  let entries = $state.raw<ChangelogEntry[] | null>(null);
  let filter = $state(initial.get("q") ?? "");
  loadChangelog().then((v) => (entries = v));

  const matches = $derived.by(() => {
    if (!entries) return [];
    const q = filter.trim().toLowerCase();
    return q
      ? entries.filter(
          (e) =>
            e.title.toLowerCase().includes(q) ||
            e.ops.some((op) => op[1].includes(q) || op[2].includes(q))
        )
      : entries;
  });

  let perPage = $state(PAGE_SIZES.includes(initialPer) ? initialPer : DEFAULT_PER_PAGE);
  let pageIndex = $state(Math.max(0, (Number(initial.get("page")) || 1) - 1));
  const pageCount = $derived(Math.max(1, Math.ceil(matches.length / perPage)));
  const current = $derived(Math.min(pageIndex, pageCount - 1));
  const shown = $derived(matches.slice(current * perPage, (current + 1) * perPage));

  let list = $state<HTMLOListElement>();
  const goTo = (p: number) => {
    pageIndex = Math.min(pageCount - 1, Math.max(0, p));
    if (list && list.getBoundingClientRect().top < 0) list.scrollIntoView({ block: "start" });
  };
  const resize = (size: number) => {
    pageIndex = Math.floor((current * perPage) / size);
    perPage = size;
  };

  $effect(() => {
    if (!entries) return;
    const next = new URLSearchParams();
    if (filter.trim()) next.set("q", filter.trim());
    if (current > 0) next.set("page", String(current + 1));
    if (perPage !== DEFAULT_PER_PAGE) next.set("per", String(perPage));
    const search = next.size ? `?${next}` : "";
    if (search !== location.search) replaceState(resolve("/changelog") + search, {});
  });

  const expanded = new SvelteSet<number>();
</script>

{#snippet pager()}
  <div
    class="flex flex-wrap items-center justify-between gap-2 font-mono text-xs text-muted select-none">
    <span>
      {#if shown.length}
        {current * perPage + 1}–{current * perPage + shown.length} of {matches.length}
      {:else}
        no matches
      {/if}
      {#if filter.trim()}({entries?.length ?? 0} in all){/if}
    </span>
    <span class="flex items-center gap-1">
      <button
        type="button"
        class="btn px-1 py-0.5"
        onclick={() => goTo(current - 1)}
        disabled={current === 0}
        aria-label="previous page"><ChevronLeft class="size-3.5" /></button>
      <span class="tabular-nums">page {current + 1} of {pageCount}</span>
      <button
        type="button"
        class="btn px-1 py-0.5"
        onclick={() => goTo(current + 1)}
        disabled={current >= pageCount - 1}
        aria-label="next page"><ChevronRight class="size-3.5" /></button>
    </span>
  </div>
{/snippet}

<svelte:head>
  <title>taxonomy changelog · atlas621</title>
</svelte:head>

<div class="flex min-h-svh flex-col">
  <Nav current="changelog">approved bulk update requests on e621, newest first</Nav>
  <main class="hex-strip relative flex-1 rounded-t-page bg-page p-3 sm:mx-4 sm:p-4">
    <div class="mx-auto max-w-3xl">
      <header class="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <h1 class="text-xl font-bold">taxonomy changelog</h1>
        <p class="text-xs text-muted">click a tag to see it on the map</p>
      </header>

      <div class="mb-3 flex flex-wrap items-center gap-x-4 gap-y-2">
        <input
          type="search"
          placeholder="filter by title or tag"
          bind:value={filter}
          oninput={() => (pageIndex = 0)}
          class="w-full max-w-sm" />
        <span class="flex items-center gap-1 text-xs text-muted select-none">
          per page
          <Select.Root
            type="single"
            value={String(perPage)}
            onValueChange={(v) => v && resize(Number(v))}>
            <Select.Trigger
              class="btn px-1.5 py-0.5 text-ink tabular-nums"
              aria-label="requests per page">
              {perPage}
              <ChevronDown class="size-3.5" />
            </Select.Trigger>
            <Select.Portal>
              <Select.Content
                sideOffset={2}
                class="z-20 min-w-(--bits-floating-anchor-width) rounded-sm border border-section-dark bg-section-light py-0.5 text-xs text-ink shadow-drop">
                {#each PAGE_SIZES as size (size)}
                  <Select.Item
                    value={String(size)}
                    label={String(size)}
                    class="cursor-pointer px-2 py-0.5 text-right tabular-nums data-highlighted:bg-section-dark data-selected:text-accent">
                    {size}
                  </Select.Item>
                {/each}
              </Select.Content>
            </Select.Portal>
          </Select.Root>
        </span>
      </div>

      {#if !entries}
        <p class="text-muted">loading</p>
      {:else}
        {@render pager()}
        <ol bind:this={list} class="my-2 scroll-mt-2 space-y-3">
          {#each shown as bur (bur.id)}
            {@const all = expanded.has(bur.id)}
            <li class="rounded-sm bg-section p-2">
              <div class="flex items-baseline justify-between gap-2">
                <h2 class="font-bold">
                  <a
                    href="https://e621.net/bulk_update_requests/{bur.id}"
                    target="_blank"
                    rel="noopener"
                    class="text-ink hover:text-link-hover"
                    title="the request on e621">{bur.title || `request #${bur.id}`}</a>
                </h2>
                <span class="shrink-0 font-mono text-xs text-muted">{bur.date}</span>
              </div>
              <ul class="mt-2 grid gap-x-4 gap-y-0.5 text-xs sm:grid-cols-2">
                {#each all ? bur.ops : bur.ops.slice(0, 40) as op, i (i)}
                  <li class="truncate">
                    <span class="text-muted">{op[0]}</span>
                    <a href={tagPath(op[1])}>{op[1]}</a>
                    {#if op[2]}
                      <ArrowRight
                        class="inline size-3 align-[-2px] text-muted"
                        aria-hidden="true" />
                      <a href={tagPath(op[2])}>{op[2]}</a>
                    {/if}
                  </li>
                {/each}
                {#if bur.ops.length > 40}
                  <li>
                    <button
                      type="button"
                      class="text-muted hover:text-link-hover"
                      onclick={() => (all ? expanded.delete(bur.id) : expanded.add(bur.id))}
                      >{all ? "fewer" : `and ${bur.ops.length - 40} more`}</button>
                  </li>
                {/if}
              </ul>
            </li>
          {/each}
        </ol>
        {@render pager()}
      {/if}
    </div>
  </main>
</div>
