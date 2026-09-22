export type Sprite = { x: number; y: number; w: number; h: number };
export type Atlas = {
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  dpr: number;
  sprites: Map<string, Sprite>;
  shelfY: number;
  shelfH: number;
  cursorX: number;
  // bumped on every recycle so a caller can tell its sprites went stale
  generation: number;
};
type Box = { width: number; height: number };
const ATLAS_SIZE = 2048;
const atlases = new Map<number, Atlas>();
const boxCache = new Map<string, Box>();
let probe: CanvasRenderingContext2D | null = null;

// 1.4 is the line height factor over the font size
export const boxHeight = (size: number, pad: number): number => Math.ceil(size * 1.4) + pad * 2;

export const measure = (key: string, text: string, font: string, pad: number): Box => {
  let box = boxCache.get(key);
  if (box) return box;
  if (boxCache.size > 20000) boxCache.clear();
  probe ??= document.createElement("canvas").getContext("2d");
  if (!probe) throw new Error("atlas: no 2d context");
  probe.font = font;
  const size = Number.parseInt(font.match(/(\d+)px/)?.[1] ?? "12", 10);
  box = {
    width: Math.ceil(probe.measureText(text).width) + pad * 2,
    height: boxHeight(size, pad)
  };
  boxCache.set(key, box);
  return box;
};

export const atlasFor = (dpr: number): Atlas => {
  let atlas = atlases.get(dpr);
  if (atlas) return atlas;
  const canvas = document.createElement("canvas");
  canvas.width = ATLAS_SIZE;
  canvas.height = ATLAS_SIZE;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("atlas: no 2d context");
  atlas = { canvas, ctx, dpr, sprites: new Map(), shelfY: 0, shelfH: 0, cursorX: 0, generation: 0 };
  atlases.set(dpr, atlas);
  return atlas;
};

export const sprite = (
  atlas: Atlas,
  key: string,
  text: string,
  font: string,
  fill: string,
  outline: string,
  pad: number
): Sprite => {
  let sp = atlas.sprites.get(key);
  if (sp) return sp;
  const { width, height } = measure(key, text, font, pad);
  const w = Math.ceil(width * atlas.dpr);
  const h = Math.ceil(height * atlas.dpr);
  if (atlas.cursorX + w > ATLAS_SIZE) {
    atlas.shelfY += atlas.shelfH;
    atlas.shelfH = 0;
    atlas.cursorX = 0;
  }
  // recycling invalidates every sprite handed out so far; callers watch generation and rebuild
  if (atlas.shelfY + Math.max(h, atlas.shelfH) > ATLAS_SIZE) {
    atlas.ctx.clearRect(0, 0, ATLAS_SIZE, ATLAS_SIZE);
    atlas.sprites.clear();
    atlas.generation++;
    atlas.shelfY = 0;
    atlas.shelfH = 0;
    atlas.cursorX = 0;
  }
  sp = { x: atlas.cursorX, y: atlas.shelfY, w, h };
  atlas.cursorX += w;
  if (h > atlas.shelfH) atlas.shelfH = h;
  const ctx = atlas.ctx;
  ctx.save();
  ctx.translate(sp.x, sp.y);
  ctx.scale(atlas.dpr, atlas.dpr);
  ctx.font = font;
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.lineWidth = 3;
  ctx.strokeStyle = outline;
  ctx.globalAlpha = 0.9;
  ctx.strokeText(text, pad, height / 2);
  ctx.globalAlpha = 1;
  ctx.fillStyle = fill;
  ctx.fillText(text, pad, height / 2);
  ctx.restore();
  atlas.sprites.set(key, sp);
  return sp;
};
