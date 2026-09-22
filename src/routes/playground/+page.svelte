<script lang="ts">
  import { replaceState } from "$app/navigation";
  import { resolve } from "$app/paths";
  import { page } from "$app/state";
  import type { Names } from "$lib/core/strings";
  import {
    loadCommunities,
    loadCore,
    loadManifest,
    loadNames,
    loadU16,
    type Communities,
    type Core
  } from "$lib/data/dataset";
  import { formatCount } from "$lib/data/search";
  import {
    blend,
    combine,
    dot,
    loadVectors,
    nearest,
    type Hit,
    type Vectors
  } from "$lib/data/vectors";
  import { CATEGORY_GENERAL, categoryCss, categoryHoverCss } from "$lib/render/palette";
  import { tagPath } from "$lib/tagpath";
  import Nav from "$lib/ui/Nav.svelte";
  import Dices from "@lucide/svelte/icons/dices";
  import ExternalLink from "@lucide/svelte/icons/external-link";

  import MapInset from "./MapInset.svelte";
  import ResultList from "./ResultList.svelte";
  import { Slot } from "./slot.svelte";

  type Loaded = {
    core: Core;
    names: Names;
    vectors: Vectors;
    region: Uint16Array;
    communities: Communities;
  };
  let data = $state.raw<Loaded | null>(null);
  let failed = $state(false);
  loadManifest()
    .then((manifest) =>
      Promise.all([
        loadCore(manifest),
        loadNames(manifest),
        loadVectors(manifest),
        loadU16(manifest, "region.bin.gz", "region", "islands"),
        loadCommunities(manifest)
      ])
    )
    .then(([core, names, vectors, region, communities]) => {
      data = { core, names, vectors, region, communities };
      placeholders = {
        q: example(names, core),
        x: pickTags(names, core, 1)[0],
        y: pickTags(names, core, 1)[0],
        a: pickTags(names, core, 1)[0],
        b: pickTags(names, core, 1)[0],
        c: pickTags(names, core, 1)[0]
      };
    })
    .catch(() => (failed = true));

  const MODES = [
    { id: "with", label: "what goes with" },
    { id: "differ", label: "how they differ" },
    { id: "analogy", label: "is to, as" }
  ] as const;
  type Mode = (typeof MODES)[number]["id"];
  const params = page.url.searchParams;
  const initialMode = params.get("m");
  let mode = $state<Mode>(MODES.some((m) => m.id === initialMode) ? (initialMode as Mode) : "with");
  const q = new Slot(params.get("q") ?? "");
  const x = new Slot(params.get("x") ?? "");
  const y = new Slot(params.get("y") ?? "");
  const a = new Slot(params.get("a") ?? "");
  const b = new Slot(params.get("b") ?? "");
  const c = new Slot(params.get("c") ?? "");
  const slots = $derived(mode === "with" ? [q] : mode === "differ" ? [x, y] : [a, b, c]);

  $effect(() => {
    const next = new URLSearchParams();
    if (mode !== "with") next.set("m", mode);
    for (const [key, slot] of [
      ["q", q],
      ["x", x],
      ["y", y],
      ["a", a],
      ["b", b],
      ["c", c]
    ] as const) {
      const text = slot.text.trim();
      if (text && slots.includes(slot)) next.set(key, text);
    }
    const search = next.size ? `?${next}` : "";
    if (search !== location.search) replaceState(resolve("/playground") + search, {});
  });

  const pickTags = (n: Names, core: Core, count: number): string[] => {
    const pool = Math.min(3000, n.length);
    const picked = new Set<number>();
    const out: string[] = [];
    while (out.length < count) {
      let i = 0;
      for (let tries = 0; tries < 20; tries++) {
        i = Math.floor(Math.random() * pool);
        if (core.categories[i] === CATEGORY_GENERAL && !picked.has(i)) break;
      }
      picked.add(i);
      out.push(n.at(i));
    }
    return out;
  };
  const example = (n: Names, core: Core): string => {
    const [p, r, s, t] = pickTags(n, core, 4);
    const forms = [
      `${p} -${r} ${s}`,
      `${p} ~${r} ~${s}`,
      `${p} ${r}`,
      `${p} -${r}`,
      `${p.slice(0, Math.max(3, Math.ceil(p.length / 2)))}* -${r}`,
      `~${p} ~${r} -${s} ${t}`
    ];
    return forms[Math.floor(Math.random() * forms.length)];
  };
  let placeholders = $state({ q: "", x: "", y: "", a: "", b: "", c: "" });
  const random = () => {
    if (!data) return;
    const { names, core } = data;
    if (mode === "with") q.text = example(names, core);
    else if (mode === "differ") [x.text, y.text] = pickTags(names, core, 2);
    else [a.text, b.text, c.text] = pickTags(names, core, 3);
  };
  const addToQuery = (name: string, sign: 1 | -1) => {
    q.text = `${q.text.trim()} ${sign < 0 ? "-" : ""}${name}`.trim();
  };

  const vec = (v: Vectors, slot: Slot) => combine(v, slot.terms);
  const exclude = $derived(new Set(slots.flatMap((s) => s.nodes)));
  type Result =
    | { kind: "list"; hits: Hit[] }
    | { kind: "differ"; alike: number; moreX: Hit[]; moreY: Hit[] };
  const result = $derived.by((): Result | null => {
    const v = data?.vectors;
    if (!v) return null;
    if (mode === "with") {
      const qv = vec(v, q);
      return qv ? { kind: "list", hits: nearest(v, qv, 25, exclude) } : null;
    }
    if (mode === "differ") {
      const vx = vec(v, x);
      const vy = vec(v, y);
      if (!vx || !vy) return null;
      const dx = blend([
        [vx, 1],
        [vy, -1]
      ]);
      const dy = blend([
        [vy, 1],
        [vx, -1]
      ]);
      return {
        kind: "differ",
        alike: dot(vx, vy),
        moreX: dx ? nearest(v, dx, 15, exclude) : [],
        moreY: dy ? nearest(v, dy, 15, exclude) : []
      };
    }
    const va = vec(v, a);
    const vb = vec(v, b);
    const vc = vec(v, c);
    if (!va || !vb || !vc) return null;
    const target = blend([
      [vc, 1],
      [vb, 1],
      [va, -1]
    ]);
    return target ? { kind: "list", hits: nearest(v, target, 25, exclude) } : null;
  });
  const lit = $derived.by(() => {
    if (!result) return [];
    if (result.kind === "list") return result.hits.map((h) => h.node);
    return [...result.moreX, ...result.moreY].map((h) => h.node);
  });
  const from = $derived(slots.flatMap((s) => s.nodes));

  const unknown = $derived(slots.flatMap((s) => s.resolved?.unknown ?? []));
  const ignored = $derived(slots.flatMap((s) => s.resolved?.ignored ?? []));
  const missing = $derived.by(() => {
    const v = data?.vectors;
    if (!v) return [];
    return slots.flatMap(
      (s) =>
        s.resolved?.terms.flatMap((t) => t.tags.filter((e) => v.row[e.i] < 0).map((e) => e.n)) ?? []
    );
  });
  const pct = (v: number) => `${(v * 100).toFixed(0)}%`;
  const label = (slot: Slot) =>
    slot.resolved?.terms.flatMap((t) => t.tags.map((e) => e.a ?? e.n)).join(" ") || "…";
</script>

{#snippet field(slot: Slot, placeholder: string, wide: boolean)}
  <input
    type="search"
    {placeholder}
    autocomplete="off"
    spellcheck="false"
    bind:value={slot.text}
    class={["min-w-0 font-mono", wide ? "flex-1" : "w-40 flex-1 sm:flex-none"]} />
{/snippet}

{#snippet chips(slot: Slot)}
  {#if slot.resolved?.terms.length}
    <p class="flex flex-wrap items-center gap-1 text-xs">
      {#each slot.resolved.terms as term, k (term.source)}
        {#if k > 0 || term.sign < 0}
          <span class="w-4 text-center font-mono text-base text-muted"
            >{term.sign > 0 ? "+" : "−"}</span>
        {/if}
        {#if term.tags.length > 1}<span class="text-muted">(</span>{/if}
        {#each term.tags as tag, j (tag.i)}
          {#if j > 0}<span class="text-muted">|</span>{/if}
          <a
            href={tagPath(tag.a ?? tag.n)}
            class={["tag-chip px-1.5 py-0.5", data && data.vectors.row[tag.i] < 0 && "opacity-50"]}
            style:--tag={categoryCss(tag.k)}
            style:--tag-alt={categoryHoverCss(tag.k)}
            title={tag.a ? `${tag.n} is an alias of ${tag.a}` : undefined}
            ><span class="text-(--tag) hover:text-(--tag-alt)">{tag.a ?? tag.n}</span></a>
        {/each}
        {#if term.tags.length > 1}<span class="text-muted">)</span>{/if}
      {/each}
    </p>
  {/if}
{/snippet}

<svelte:head>
  <title>playground · atlas621</title>
</svelte:head>

<div class="flex min-h-svh flex-col">
  <Nav current="playground">questions the map can answer</Nav>
  <main class="hex-strip relative flex-1 rounded-t-page bg-page p-3 sm:mx-4 sm:p-4">
    <div class="mx-auto max-w-4xl">
      <header class="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <h1 class="text-xl font-bold">playground</h1>
        <p class="text-xs text-muted">
          e621 search syntax:
          <a
            href="https://e621.net/help/cheatsheet"
            target="_blank"
            rel="noopener"
            class="text-link hover:text-link-hover"
            >cheatsheet <ExternalLink class="inline size-3" aria-hidden="true" /></a>
        </p>
      </header>

      <menu class="mb-3 flex flex-wrap gap-1 text-xs">
        {#each MODES as m (m.id)}
          <li>
            <button
              type="button"
              class={[
                "btn",
                mode === m.id && "bg-field font-bold text-field-ink hover:bg-field active:bg-field"
              ]}
              aria-pressed={mode === m.id}
              onclick={() => (mode = m.id)}>{m.label}</button>
          </li>
        {/each}
      </menu>

      <p class="mb-1.5 text-xs text-muted">
        {#if mode === "with"}
          like an e621 search, but instead of posts you get tags: the ones a post matching your
          search would most likely also carry, with a % for how well each fits. <code>-tag</code>
          pushes away from a tag instead of just hiding it, <code>~a ~b</code> means either,
          <code>skunk*</code> is every tag that matches.
        {:else if mode === "differ"}
          what sets the first apart from the second and the second from the first, plus how alike
          the two are overall. each side takes the same search syntax.
        {:else}
          the tag that relates to the third the way the second relates to the first: skunk is to
          fart as horse is to what.
        {/if}
      </p>

      <div class="mb-1.5 flex flex-wrap items-center gap-1.5">
        {#if mode === "with"}
          {@render field(q, placeholders.q, true)}
        {:else if mode === "differ"}
          {@render field(x, placeholders.x, false)}
          <span class="text-xs text-muted">versus</span>
          {@render field(y, placeholders.y, false)}
        {:else}
          {@render field(a, placeholders.a, false)}
          <span class="text-xs text-muted">is to</span>
          {@render field(b, placeholders.b, false)}
          <span class="text-xs text-muted">as</span>
          {@render field(c, placeholders.c, false)}
          <span class="text-xs text-muted">is to …</span>
        {/if}
        <button
          type="button"
          class="btn shrink-0 px-1.5"
          disabled={!data}
          onclick={random}
          aria-label="random"
          title="random"><Dices class="size-4" /></button>
      </div>

      {#if failed}
        <p class="text-xs text-muted">the data could not be loaded</p>
      {:else if !data}
        <p class="text-xs text-muted">loading</p>
      {:else}
        <div class="mb-3 flex flex-col gap-1">
          {#if mode === "with"}
            {@render chips(q)}
          {/if}
          {#if unknown.length}
            <p class="text-xs text-muted">not a tag: {unknown.join(", ")}</p>
          {/if}
          {#if ignored.length}
            <p class="text-xs text-muted">no meaning here, ignored: {ignored.join(", ")}</p>
          {/if}
          {#if missing.length}
            <p class="text-xs text-muted">
              under {formatCount(data.core.manifest.playground_floor)} posts, no vector, left out:
              {missing.join(", ")}
            </p>
          {/if}
        </div>

        {#if result}
          <div class="flex flex-col gap-4 sm:flex-row sm:items-start">
            <div class="min-w-0 flex-1">
              {#if result.kind === "list"}
                <ResultList
                  hits={result.hits}
                  core={data.core}
                  names={data.names}
                  region={data.region}
                  communities={data.communities}
                  onadd={mode === "with" ? addToQuery : undefined} />
              {:else}
                <p class="mb-2 text-xs">
                  <span class="text-muted">overall the two are</span>
                  <span class="font-bold tabular-nums">{pct(result.alike)}</span>
                  <span class="text-muted">alike</span>
                </p>
                <div class="flex flex-col gap-4 sm:flex-row">
                  <div class="min-w-0 flex-1">
                    <h3 class="mb-1 text-sm font-bold">more {label(x)} than {label(y)}</h3>
                    <ResultList
                      hits={result.moreX}
                      core={data.core}
                      names={data.names}
                      region={data.region}
                      communities={data.communities} />
                  </div>
                  <div class="min-w-0 flex-1">
                    <h3 class="mb-1 text-sm font-bold">more {label(y)} than {label(x)}</h3>
                    <ResultList
                      hits={result.moreY}
                      core={data.core}
                      names={data.names}
                      region={data.region}
                      communities={data.communities} />
                  </div>
                </div>
              {/if}
            </div>
            <div class="shrink-0">
              <MapInset
                positions={data.core.positions}
                space={data.core.manifest.space_size}
                {lit}
                {from} />
              <p class="mt-1 text-xs text-muted">white: what you typed. yellow: the answers.</p>
            </div>
          </div>
        {/if}
      {/if}
    </div>
  </main>
</div>
