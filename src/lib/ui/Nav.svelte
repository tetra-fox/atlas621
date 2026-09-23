<script lang="ts">
  import { resolve } from "$app/paths";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import type { Snippet } from "svelte";

  type Props = { current: "map" | "playground" | "changelog"; children?: Snippet };
  let { current, children }: Props = $props();

  // the playground route still resolves, it just does not get a tab
  const sections = [
    { id: "map", label: "map", href: resolve("/(map)") },
    { id: "changelog", label: "changelog", href: resolve("/changelog") }
  ] as const;

  const links = [
    { label: "e621", href: "https://e621.net", end: false },
    { label: "source", href: "https://github.com/tetra-fox/atlas621", end: true }
  ] as const;
</script>

<nav class="flex items-end gap-2 px-2 pt-1 pb-1.5 sm:px-4">
  <a
    href={resolve("/(map)")}
    class="shrink-0 hover:brightness-125 active:brightness-90"
    aria-label="atlas621, the map">
    <svg viewBox="0 0 64 64" class="h-13 w-13" aria-hidden="true">
      <path d="M6 32L19 9.5H45L58 32L45 54.5H19Z" class="fill-mark" />
      <text x="32" y="30" text-anchor="middle" font-size="18" font-weight="bold" class="fill-ink"
        >a</text>
      <text x="32" y="48" text-anchor="middle" font-size="11" font-weight="bold" class="fill-ink"
        >621</text>
    </svg>
  </a>
  <div class="flex min-w-0 flex-1 flex-col">
    <menu class="flex h-6 items-center select-none">
      {#each sections as s (s.id)}
        <li>
          <a
            href={s.href}
            data-label={s.label}
            class={[
              "flex h-6 flex-col items-center justify-center rounded-t-sm px-2.5",
              "after:invisible after:h-0 after:overflow-hidden after:font-bold after:content-[attr(data-label)]",
              current === s.id ? "bg-page font-bold text-ink" : "hover:bg-page/60 active:bg-page"
            ]}
            aria-current={current === s.id ? "page" : undefined}>{s.label}</a>
        </li>
      {/each}
      {#each links as l (l.href)}
        <li class={[l.end && "ml-auto"]}>
          <a
            href={l.href}
            target="_blank"
            rel="noopener"
            class="flex h-6 items-center gap-1 rounded-t-sm px-2.5 hover:bg-page/60 active:bg-page"
            >{l.label} <ExternalLink class="size-3" aria-hidden="true" /></a>
        </li>
      {/each}
    </menu>
    <div
      class={[
        "flex h-7 min-w-0 items-center gap-2 rounded-sm bg-page px-2.5 text-xs text-muted",
        current === sections[0].id && "rounded-tl-none"
      ]}>
      {@render children?.()}
    </div>
  </div>
</nav>
