export const glRenderer = (): string => {
  const canvas = document.createElement("canvas");
  const gl = canvas.getContext("webgl2") ?? canvas.getContext("webgl");
  if (!gl) return "no webgl";
  const info = gl.getExtension("WEBGL_debug_renderer_info");
  const renderer = info
    ? String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL))
    : String(gl.getParameter(gl.RENDERER));
  const kind = gl instanceof WebGL2RenderingContext ? "webgl2" : "webgl1";
  return `${kind} · ${renderer}`;
};

const listeners = new Set<(now: number, gap: number) => void>();
let handle = 0;
let previous = 0;
const tick = (now: number) => {
  const gap = previous === 0 ? 0 : now - previous;
  previous = now;
  for (const fn of listeners) fn(now, gap);
  handle = requestAnimationFrame(tick);
};

const onVisibility = () => {
  previous = 0;
};

export const watchFrames = (fn: (now: number, gap: number) => void): (() => void) => {
  listeners.add(fn);
  if (!handle) {
    previous = 0;
    document.addEventListener("visibilitychange", onVisibility);
    handle = requestAnimationFrame(tick);
  }
  return () => {
    listeners.delete(fn);
    if (listeners.size === 0) {
      cancelAnimationFrame(handle);
      document.removeEventListener("visibilitychange", onVisibility);
      handle = 0;
    }
  };
};

export const startFpsMeter = (report: (fps: number) => void): (() => void) => {
  let frames = 0;
  let last = performance.now();
  return watchFrames((now) => {
    frames++;
    if (now - last < 1000) return;
    report(Math.round((frames * 1000) / (now - last)));
    frames = 0;
    last = now;
  });
};

const HITCH_MS = 50;

export type Hitch = { at: number; ms: number };

export const startHitchWatch = (report: (hitch: Hitch) => void): (() => void) =>
  watchFrames((now, gap) => {
    if (gap >= HITCH_MS) report({ at: now - gap, ms: gap });
  });
