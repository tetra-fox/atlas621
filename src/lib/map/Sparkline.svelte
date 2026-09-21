<script lang="ts">
  type Props = { values: number[]; years: number[]; highlight?: [number, number] | null };
  let { values, years, highlight = null }: Props = $props();

  let w = $state(220);
  const h = 40;
  const max = $derived(Math.max(1, ...values));
  const points = $derived(
    values
      .map(
        (v, i) =>
          `${((i / Math.max(1, values.length - 1)) * w).toFixed(1)},${(h - (v / max) * h).toFixed(1)}`
      )
      .join(" ")
  );
  const peak = $derived(values.indexOf(max));
  const x = (i: number) => (i / Math.max(1, values.length - 1)) * w;
</script>

<div bind:clientWidth={w}>
  <svg
    viewBox="0 0 {w} {h}"
    class="h-10 w-full overflow-visible"
    role="img"
    aria-label="posts per year">
    <polyline {points} fill="none" stroke="currentColor" stroke-width="1.5" class="text-accent" />
    {#if highlight !== null && years.includes(highlight[0]) && years.includes(highlight[1])}
      {@const from = years.indexOf(highlight[0])}
      {@const to = years.indexOf(highlight[1])}
      {#if to > from}
        <rect x={x(from)} width={x(to) - x(from)} y="0" height={h} class="fill-muted/20" />
      {/if}
      {#each new Set([from, to]) as i (i)}
        <line
          x1={x(i)}
          x2={x(i)}
          y1="0"
          y2={h}
          stroke="currentColor"
          class="text-muted"
          stroke-dasharray="2 2" />
      {/each}
    {/if}
    <text x={x(peak)} y="-3" text-anchor="middle" class="fill-muted text-[8px]"
      >{years[peak]}: {max}</text>
  </svg>
</div>
