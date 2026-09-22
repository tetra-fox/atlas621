// flat-top axial hexes of circumradius size: columns are 1.5 apart, rows sqrt3, and a cell
// spans 2 * size across by sqrt3 * size down. must agree with Grid in the pipeline's
// territories.rs
const SQRT3 = Math.sqrt(3);

type Grid = { size: number; origin: [number, number] };

export const hexCenter = (grid: Grid, q: number, r: number): [number, number] => [
  grid.size * 1.5 * q + grid.origin[0],
  grid.size * SQRT3 * (r + q / 2) + grid.origin[1]
];

export const hexHalfHeight = (grid: Grid): number => (grid.size * SQRT3) / 2;

// corner offsets from a cell centre, counterclockwise from the +x vertex
export const hexCorners = (grid: Grid): { dx: number[]; dy: number[] } => {
  const r = grid.size;
  const h = hexHalfHeight(grid);
  return { dx: [r, r / 2, -r / 2, -r, -r / 2, r / 2], dy: [0, h, h, 0, -h, -h] };
};

// the six axial steps to a cell's neighbours, in the same order as hexCorners
export const HEX_ACROSS = [
  [1, 0],
  [0, 1],
  [-1, 1],
  [-1, 0],
  [0, -1],
  [1, -1]
];

// cells ship as a flat [q, r, q, r, ...] run; a callback rather than a generator because these
// runs reach tens of thousands of cells and generators allocate per step
export const eachHexCell = (hexes: number[], fn: (q: number, r: number) => void): void => {
  for (let i = 0; i < hexes.length; i += 2) fn(hexes[i], hexes[i + 1]);
};
