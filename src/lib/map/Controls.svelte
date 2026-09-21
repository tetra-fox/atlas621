<script lang="ts">
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import { RadioGroup, Slider, Switch, Toggle } from "bits-ui";
  import type { Attachment } from "svelte/attachments";

  import { MAX_DENSITY } from "./linkselect";
  import { CATEGORY_NAMES, categoryCss, categoryHoverCss } from "./palette";
  import { getMapState } from "./state.svelte";

  type Props = { years: number[] };
  let { years }: Props = $props();
  const ui = getMapState();

  const first = $derived(years[0]);
  const last = $derived(years[years.length - 1]);

  const DRAG_SLOP = 3;

  const draggable =
    (set: (v: boolean) => void): Attachment<HTMLElement> =>
    (root) => {
      let swallowClick = false;
      const onClick = (click: MouseEvent) => {
        if (!swallowClick) return;
        swallowClick = false;
        click.stopPropagation();
      };
      const onPointerDown = (down: PointerEvent) => {
        if (down.button !== 0) return;
        const thumb = root.querySelector<HTMLElement>("[data-thumb]");
        if (!thumb) return;
        const rect = thumb.getBoundingClientRect();
        const travel = root.clientWidth - thumb.offsetWidth;
        const track = rect.left - thumb.offsetLeft;
        const grip =
          rect.left <= down.clientX && down.clientX < rect.right
            ? down.clientX - rect.left
            : rect.width / 2;
        let moving = false;
        let picked = false;
        const cleanup = () => {
          window.removeEventListener("pointermove", onMove);
          window.removeEventListener("pointerup", onUp);
          window.removeEventListener("pointercancel", onCancel);
          if (!moving) return;
          thumb.style.left = "";
          thumb.style.transition = "";
        };
        const finish = (swallow: boolean) => {
          cleanup();
          if (!moving) return;
          swallowClick = swallow;
          set(picked);
        };
        const onMove = (move: PointerEvent) => {
          if (move.pointerId !== down.pointerId) return;
          if (!(move.buttons & 1)) return finish(false);
          if (!moving) {
            if (Math.abs(move.clientX - down.clientX) < DRAG_SLOP) return;
            moving = true;
            root.setPointerCapture(down.pointerId);
            thumb.style.transition = "none";
          }
          const left = Math.min(travel, Math.max(0, move.clientX - track - grip));
          picked = left > travel / 2;
          thumb.style.left = `${left}px`;
        };
        const onUp = (up: PointerEvent) => {
          if (up.pointerId === down.pointerId) finish(true);
        };
        const onCancel = (cancel: PointerEvent) => {
          if (cancel.pointerId === down.pointerId) cleanup();
        };
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
        window.addEventListener("pointercancel", onCancel);
      };
      root.addEventListener("pointerdown", onPointerDown);
      root.addEventListener("click", onClick, { capture: true });
      return () => {
        root.removeEventListener("pointerdown", onPointerDown);
        root.removeEventListener("click", onClick, { capture: true });
      };
    };

  let playing = $state(false);
  let timer: ReturnType<typeof setInterval> | undefined;
  const stop = () => {
    clearInterval(timer);
    timer = undefined;
    playing = false;
  };
  const togglePlay = () => {
    if (playing) return stop();
    if (ui.yearRange === null) ui.yearRange = [first, first];
    playing = true;
    timer = setInterval(() => {
      const [from, to] = ui.yearRange ?? [first, first - 1];
      if (to + 1 > last) {
        ui.yearRange = null;
        stop();
      } else {
        ui.yearRange = [from, to + 1];
      }
    }, 700);
  };
  const setYears = (v: number[]) => {
    ui.yearRange = v[0] <= first && v[1] >= last ? null : [v[0], v[1]];
  };
  $effect(() => () => clearInterval(timer));
</script>

{#snippet slider(
  label: string,
  min: number,
  max: number,
  step: number,
  get: () => number,
  set: (v: number) => void
)}
  <Slider.Root
    type="single"
    bind:value={get, set}
    {min}
    {max}
    {step}
    class="relative flex h-4 flex-1 touch-none items-center select-none">
    <span class="relative h-3 w-full overflow-hidden rounded-xs bg-ground inset-shadow-track">
      <Slider.Range class="absolute h-full bg-section-lighter" />
    </span>
    <Slider.Thumb
      index={0}
      aria-label={label}
      class="block size-4 rounded-xs bg-accent hover:scale-110 hover:bg-link-hover active:scale-125 data-active:scale-125" />
  </Slider.Root>
{/snippet}

{#snippet rangeSlider(
  labels: [string, string],
  min: number,
  max: number,
  get: () => number[],
  set: (v: number[]) => void,
  readout: boolean
)}
  {@const values = get()}
  {@const close = values[1] - values[0] < 4}
  <Slider.Root
    type="multiple"
    bind:value={get, set}
    {min}
    {max}
    step={1}
    class="relative flex h-4 flex-1 touch-none items-center select-none">
    <span class="relative h-3 w-full overflow-hidden rounded-xs bg-ground inset-shadow-track">
      <Slider.Range class="absolute h-full bg-section-lighter" />
    </span>
    {#each labels as label, index (label)}
      <Slider.Thumb
        {index}
        aria-label={label}
        class="block size-4 rounded-xs bg-accent hover:scale-110 hover:bg-link-hover active:scale-125 data-active:scale-125" />
      {#if readout && (index === 1 || values[0] !== values[1])}
        <Slider.ThumbLabel
          {index}
          position="top"
          class={[
            "pointer-events-none mb-0.5 text-[10px] leading-none whitespace-nowrap text-ink/70 tabular-nums",
            close &&
              values[0] !== values[1] &&
              (index === 0 ? "!-translate-x-full" : "!translate-x-0")
          ]}>
          {values[index]}
        </Slider.ThumbLabel>
      {/if}
    {/each}
  </Slider.Root>
{/snippet}

{#snippet toggle(label: string, on: boolean, set: (v: boolean) => void)}
  <label class="group flex cursor-pointer items-center gap-2">
    <Switch.Root
      checked={on}
      onCheckedChange={set}
      {@attach draggable(set)}
      class="relative h-6 w-10 shrink-0 touch-pan-y select-none">
      <span class="absolute inset-x-0 top-1.5 h-3 rounded-xs bg-ground inset-shadow-track"></span>
      <Switch.Thumb
        data-thumb="true"
        class="absolute top-1 left-0 size-4 rounded-xs bg-muted transition-[left,background-color,filter] group-hover:brightness-125 group-active:brightness-150 data-[state=checked]:left-6 data-[state=checked]:bg-accent" />
    </Switch.Root>
    {label}
  </label>
{/snippet}

<div class="flex flex-col gap-3 text-xs select-none">
  <section>
    <h2 class="mb-1 text-base font-bold">categories</h2>
    <ul class="flex flex-wrap gap-1">
      {#each CATEGORY_NAMES as name, k (name)}
        {#if k !== 6}
          <li>
            <Toggle.Root
              bind:pressed={ui.categoriesOn[k]}
              class="tag-chip px-1.5 py-0.5 text-(--tag) hover:text-(--tag-alt)"
              style="--tag: {ui.categoriesOn[k]
                ? categoryCss(k)
                : 'var(--color-muted)'}; --tag-alt: {categoryHoverCss(k)}">
              {name}
            </Toggle.Root>
          </li>
        {/if}
      {/each}
    </ul>
  </section>

  <section class="flex flex-col gap-2">
    <h2 class="text-base font-bold">view</h2>
    <div class="flex items-center gap-2">
      <span class="w-12 text-ink/70">edges</span>
      {@render slider(
        "edge density",
        0,
        MAX_DENSITY,
        1,
        () => ui.edgeDensity,
        (v) => (ui.edgeDensity = v)
      )}
    </div>
    <div class="mt-1 flex items-center gap-2">
      <span class="w-12 text-ink/70">years</span>
      {@render rangeSlider(
        ["first year", "last year"],
        first,
        last,
        () => ui.yearRange ?? [first, last],
        setYears,
        ui.yearRange !== null
      )}
      <button
        type="button"
        class="btn px-1 py-0.5"
        onclick={togglePlay}
        aria-label={playing ? "pause" : "play years"}>
        {#if playing}<Pause class="size-3" />{:else}<Play class="size-3" />{/if}
      </button>
    </div>
    <div class="flex flex-wrap gap-x-4 gap-y-1">
      {@render toggle("implications", ui.showImplications, (v) => (ui.showImplications = v))}
      {@render toggle("labels", ui.labels, (v) => (ui.labels = v))}
      {@render toggle("territories", ui.territories, (v) => (ui.territories = v))}
    </div>
    <div class="flex items-center gap-2">
      <span class="w-12 text-ink/70">color</span>
      <RadioGroup.Root
        orientation="horizontal"
        aria-label="color points by"
        bind:value={
          () => ui.colorMode, (v) => (ui.colorMode = v === "birth" ? "birth" : "category")
        }
        {@attach draggable((v) => (ui.colorMode = v ? "birth" : "category"))}
        class="relative flex flex-1 touch-pan-y rounded-sm border border-section-light bg-ground inset-shadow-track select-none">
        <span
          data-thumb="true"
          data-state={ui.colorMode}
          class="absolute inset-y-0 left-0 w-1/2 rounded-sm bg-accent transition-[left] data-[state=birth]:left-1/2"
        ></span>
        <RadioGroup.Item
          value="category"
          class="relative z-10 flex-1 rounded-sm py-1 transition-colors not-data-[state=checked]:hover:text-link-hover data-[state=checked]:text-black"
          >category</RadioGroup.Item>
        <RadioGroup.Item
          value="birth"
          class="relative z-10 flex-1 rounded-sm py-1 transition-colors not-data-[state=checked]:hover:text-link-hover data-[state=checked]:text-black"
          >birth year</RadioGroup.Item>
      </RadioGroup.Root>
    </div>
  </section>
</div>
