import { describe, expect, it } from "vitest";

import { eachHexCell, HEX_ACROSS, hexCenter, hexCorners, hexHalfHeight } from "./hex";

const grid = { size: 3, origin: [10, 20] as [number, number] };

describe("hexCenter", () => {
  it("puts the origin cell at the grid origin", () => {
    expect(hexCenter(grid, 0, 0)).toEqual([10, 20]);
  });

  it("matches the flat-top pitch: 1.5 across a column, sqrt3 down a row", () => {
    const [x] = hexCenter(grid, 1, 0);
    const [, y] = hexCenter(grid, 0, 1);
    expect(x - 10).toBeCloseTo(4.5, 12);
    expect(y - 20).toBeCloseTo(3 * Math.sqrt(3), 12);
  });

  it("offsets every other column by half a row", () => {
    const [, even] = hexCenter(grid, 0, 0);
    const [, odd] = hexCenter(grid, 1, 0);
    expect(odd - even).toBeCloseTo(hexHalfHeight(grid), 12);
  });

  it("keeps each neighbour one cell away", () => {
    const [cx, cy] = hexCenter(grid, 4, -2);
    for (const [dq, dr] of HEX_ACROSS) {
      const [nx, ny] = hexCenter(grid, 4 + dq, -2 + dr);
      expect(Math.hypot(nx - cx, ny - cy)).toBeCloseTo(grid.size * Math.sqrt(3), 12);
    }
  });
});

describe("hexCorners", () => {
  it("returns six corners one circumradius from the centre", () => {
    const { dx, dy } = hexCorners(grid);
    expect(dx).toHaveLength(6);
    expect(dy).toHaveLength(6);
    for (let k = 0; k < 6; k++) expect(Math.hypot(dx[k], dy[k])).toBeCloseTo(grid.size, 12);
  });

  it("starts at the +x vertex, which is what makes it flat-top", () => {
    const { dx, dy } = hexCorners(grid);
    expect([dx[0], dy[0]]).toEqual([grid.size, 0]);
    expect(dy[1]).toBeCloseTo(hexHalfHeight(grid), 12);
  });
});

describe("eachHexCell", () => {
  it("reads a flat run as q, r pairs", () => {
    const seen: [number, number][] = [];
    eachHexCell([1, 2, -3, 4], (q, r) => seen.push([q, r]));
    expect(seen).toEqual([
      [1, 2],
      [-3, 4]
    ]);
  });

  it("yields nothing for an empty run", () => {
    let calls = 0;
    eachHexCell([], () => calls++);
    expect(calls).toBe(0);
  });
});
