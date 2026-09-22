<script lang="ts">
  type Props = { positions: Float32Array; space: number; lit: number[]; from: number[] };
  let { positions, space, lit, from }: Props = $props();

  const SIZE = 300;
  let canvas = $state<HTMLCanvasElement>();
  let base: HTMLCanvasElement | null = null;
  let basedOn: Float32Array | null = null;
  let baseDpr = 0;

  const ground = (dpr: number): HTMLCanvasElement => {
    if (base && basedOn === positions && baseDpr === dpr) return base;
    const px = Math.round(SIZE * dpr);
    const off = document.createElement("canvas");
    off.width = px;
    off.height = px;
    const ctx = off.getContext("2d");
    if (!ctx) throw new Error("map inset: no 2d context");
    const density = new Float32Array(px * px);
    const n = positions.length / 2;
    for (let i = 0; i < n; i++) {
      const x = Math.floor((positions[i * 2] / space) * px);
      const y = Math.floor((1 - positions[i * 2 + 1] / space) * px);
      if (x >= 0 && x < px && y >= 0 && y < px) density[y * px + x]++;
    }
    const img = ctx.createImageData(px, px);
    for (let k = 0; k < density.length; k++) {
      const d = density[k];
      if (d === 0) continue;
      const a = Math.min(1, 0.25 + Math.sqrt(d) * 0.18);
      img.data[k * 4] = 150;
      img.data[k * 4 + 1] = 170;
      img.data[k * 4 + 2] = 210;
      img.data[k * 4 + 3] = Math.round(a * 255);
    }
    ctx.putImageData(img, 0, 0);
    base = off;
    basedOn = positions;
    baseDpr = dpr;
    return off;
  };

  const dot = (ctx: CanvasRenderingContext2D, node: number, r: number, fill: string) => {
    const x = (positions[node * 2] / space) * SIZE;
    const y = (1 - positions[node * 2 + 1] / space) * SIZE;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fillStyle = fill;
    ctx.fill();
  };

  $effect(() => {
    const el = canvas;
    if (!el) return;
    const dpr = window.devicePixelRatio || 1;
    const px = Math.round(SIZE * dpr);
    if (el.width !== px || el.height !== px) {
      el.width = px;
      el.height = px;
    }
    const ctx = el.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, px, px);
    ctx.drawImage(ground(dpr), 0, 0);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    for (const node of lit) dot(ctx, node, 4.5, "rgba(0,0,0,0.6)");
    for (const node of lit) dot(ctx, node, 3, "rgb(255,220,90)");
    for (const node of from) {
      dot(ctx, node, 5, "rgba(0,0,0,0.6)");
      dot(ctx, node, 3.5, "rgb(255,255,255)");
    }
  });
</script>

<canvas
  bind:this={canvas}
  class="aspect-square w-full max-w-75 rounded-sm bg-page-dim"
  style:width="{SIZE}px"
  style:height="{SIZE}px"
  aria-label="where the results sit on the map"></canvas>
