// indices are e621's own tag category ids
export const CATEGORY_NAMES = [
  "general",
  "artist",
  "contributor",
  "copyright",
  "character",
  "species",
  "invalid",
  "meta",
  "lore"
] as const;
export const CATEGORY_COUNT = CATEGORY_NAMES.length;
export const CATEGORY_GENERAL = 0;

export const CATEGORY_COLORS: [number, number, number][] = [
  [180, 199, 217],
  [242, 172, 8],
  [192, 192, 192],
  [221, 0, 221],
  [0, 170, 0],
  [237, 93, 31],
  [255, 61, 61],
  [255, 255, 255],
  [34, 136, 34]
];

const CATEGORY_HOVER_COLORS: [number, number, number][] = [
  [46, 118, 180],
  [251, 214, 127],
  [113, 112, 110],
  [255, 94, 255],
  [43, 255, 43],
  [246, 178, 149],
  [255, 189, 189],
  [102, 102, 102],
  [95, 219, 95]
];

const css = ([r, g, b]: [number, number, number]): string => `rgb(${r} ${g} ${b})`;
export const categoryCss = (k: number): string => css(CATEGORY_COLORS[k] ?? CATEGORY_COLORS[0]);
export const categoryHoverCss = (k: number): string =>
  css(CATEGORY_HOVER_COLORS[k] ?? CATEGORY_HOVER_COLORS[0]);

export const radiusFor = (count: number): number =>
  Math.min(40, Math.max(1.5, 0.7 * Math.pow(count, 0.28)));

export const sizeScaleFor = (ppu: number): number => Math.min(4, Math.max(1, Math.sqrt(ppu / 5)));

export const yearColor = (year: number, first: number, last: number): [number, number, number] => {
  const t = Math.min(1, Math.max(0, (year - first) / Math.max(1, last - first)));
  const stops: [number, number, number][] = [
    [255, 176, 64],
    [240, 96, 96],
    [170, 90, 200],
    [70, 140, 255],
    [120, 230, 240]
  ];
  const f = t * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(f));
  const u = f - i;
  return [0, 1, 2].map((c) => Math.round(stops[i][c] * (1 - u) + stops[i + 1][c] * u)) as [
    number,
    number,
    number
  ];
};

export const communityColor = (index: number): [number, number, number] => {
  const h = (index * 0.61803398875) % 1;
  const s = 0.55;
  const v = 0.9;
  const i = Math.floor(h * 6);
  const f = h * 6 - i;
  const p = v * (1 - s);
  const q = v * (1 - f * s);
  const t = v * (1 - (1 - f) * s);
  const [r, g, b] = [
    [v, t, p],
    [q, v, p],
    [p, v, t],
    [p, q, v],
    [t, p, v],
    [v, p, q]
  ][i % 6];
  return [Math.round(r * 255), Math.round(g * 255), Math.round(b * 255)];
};
