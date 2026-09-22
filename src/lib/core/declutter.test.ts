import { describe, expect, it } from "vitest";

import { labelZooms, zoomBucket, ZOOM_BUCKETS } from "./declutter";

const SPACE = 4096;
const GAP_Y = 16;
const ZOOM_MAX = 300;

type Scene = { positions: Float32Array; reach: Float32Array; halfH: Float32Array };

const solve = (s: Scene, zoomMax = ZOOM_MAX) =>
  labelZooms(s.positions, s.reach, s.halfH, GAP_Y, SPACE, zoomMax, () => {});

// a label sits at its point and extends rightward by reach, and half its box height either way.
// two are clear when the zoom has pushed them apart on either axis
const clear = (s: Scene, i: number, j: number, zoom: number) => {
  const dx = (s.positions[i * 2] - s.positions[j * 2]) * zoom;
  const dy = Math.abs(s.positions[i * 2 + 1] - s.positions[j * 2 + 1]) * zoom;
  const apartX = dx > 0 ? dx >= s.reach[j] : -dx >= s.reach[i];
  return apartX || dy >= s.halfH[i] + s.halfH[j] + GAP_Y;
};

const scene = (pts: [number, number][], reach = 80, halfH = 12): Scene => ({
  positions: Float32Array.from(pts.flat()),
  reach: Float32Array.from(pts, () => reach),
  halfH: Float32Array.from(pts, () => halfH)
});

describe("zoomBucket", () => {
  it("never leaves the table", () => {
    for (const z of [-1, 0, 1e-9, 0.01, 1, 300, 1e9]) {
      const b = zoomBucket(z);
      expect(b).toBeGreaterThanOrEqual(0);
      expect(b).toBeLessThan(ZOOM_BUCKETS);
    }
  });

  it("puts a never-shown label past every real bucket", () => {
    expect(zoomBucket(Infinity)).toBe(ZOOM_BUCKETS);
    expect(zoomBucket(Infinity)).toBeGreaterThan(zoomBucket(1e9));
  });

  it("rises with zoom", () => {
    const zooms = [0.05, 0.5, 1, 4, 32, 256];
    const buckets = zooms.map(zoomBucket);
    expect(buckets).toEqual([...buckets].sort((a, b) => a - b));
  });
});

describe("labelZooms", () => {
  it("shows a lone label at every zoom", () => {
    expect(solve(scene([[100, 100]])).zoom[0]).toBe(0);
  });

  // every label piles onto one point as zoom goes to zero, so a threshold is never 0 for a
  // second label; what matters is that it is exactly where the pair comes apart
  it("puts the threshold exactly where the pair separates", () => {
    const s = scene([
      [0, 0],
      [2000, 2000]
    ]);
    // thresholds are stored as f32, so compare a hair either side rather than exactly on it
    const t = solve(s).zoom[1];
    expect(clear(s, 1, 0, t * 1.0001)).toBe(true);
    expect(clear(s, 1, 0, t * 0.99)).toBe(false);
  });

  it("clears distant labels at a far lower zoom than close ones", () => {
    const distant = solve(
      scene([
        [0, 0],
        [2000, 2000]
      ])
    ).zoom[1];
    const close = solve(
      scene([
        [0, 0],
        [20, 20]
      ])
    ).zoom[1];
    expect(distant).toBeLessThan(close / 10);
  });

  it("keeps the earlier label and defers the one behind it", () => {
    const { zoom } = solve(
      scene([
        [100, 100],
        [101, 100]
      ])
    );
    expect(zoom[0]).toBe(0);
    expect(zoom[1]).toBeGreaterThan(0);
  });

  it("defers a closer neighbour to a higher zoom than a distant one", () => {
    const near = solve(
      scene([
        [100, 100],
        [101, 100]
      ])
    ).zoom[1];
    const far = solve(
      scene([
        [100, 100],
        [140, 100]
      ])
    ).zoom[1];
    expect(near).toBeGreaterThan(far);
  });

  it("hides a label that cannot clear before the zoom ceiling", () => {
    const { zoom } = solve(
      scene([
        [100, 100],
        [100.0001, 100]
      ]),
      10
    );
    expect(zoom[1]).toBe(Infinity);
  });

  // the property the whole module exists for: whatever zoom you pick, the labels it says are
  // visible must not collide
  it("never lets two visible labels overlap", () => {
    let seed = 12345;
    const rand = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
    const pts: [number, number][] = Array.from({ length: 400 }, () => [rand() * 600, rand() * 600]);
    const s: Scene = {
      positions: Float32Array.from(pts.flat()),
      reach: Float32Array.from(pts, () => 20 + rand() * 90),
      halfH: Float32Array.from(pts, () => 8 + rand() * 8)
    };
    const { zoom } = solve(s, 1e6);

    for (const z of [0.25, 1, 2, 5, 12, 40, 150]) {
      const visible = [...zoom.keys()].filter((i) => zoom[i] <= z);
      for (let a = 0; a < visible.length; a++)
        for (let b = a + 1; b < visible.length; b++) {
          const [i, j] = [visible[a], visible[b]];
          expect(
            clear(s, i, j, z),
            `labels ${i} and ${j} both visible at zoom ${z} but overlap`
          ).toBe(true);
        }
    }
  });

  it("orders and indexes every label exactly once", () => {
    const pts: [number, number][] = Array.from({ length: 120 }, (_, i) => [
      (i % 12) * 30,
      Math.floor(i / 12) * 18
    ]);
    const { zoom, order, starts } = solve(scene(pts));
    expect(order).toHaveLength(pts.length);
    expect(new Set(order).size).toBe(pts.length);
    expect(starts[ZOOM_BUCKETS + 1]).toBe(pts.length);

    // order is grouped by bucket, so the prefix for a zoom is exactly the labels at or below it
    for (const z of [0.5, 2, 20]) {
      const prefix = Array.from(order.subarray(0, starts[zoomBucket(z) + 1]));
      expect(prefix.every((i) => zoom[i] <= z || zoomBucket(zoom[i]) <= zoomBucket(z))).toBe(true);
    }
  });

  it("reports the widest reach, which the caller uses to pad its viewport query", () => {
    const s = scene([
      [0, 0],
      [500, 500]
    ]);
    s.reach[1] = 250;
    expect(solve(s).reach).toBe(250);
  });
});
