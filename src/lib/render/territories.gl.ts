import { eachHexCell, HEX_ACROSS, hexCenter, hexCorners } from "$lib/core/hex";
import type { TerritoryLevel } from "$lib/data/dataset";

import { communityColor } from "./palette";

const BITS = 15;
const OFF = 1 << (BITS - 1);
const cellKey = (q: number, r: number) => ((q + OFF) << BITS) | (r + OFF);

const FILL_ALPHA = 0.09;
const LINE_ALPHA = 0.35;

const VERTEX = `#version 300 es
in vec2 pos;
in vec2 dir;
in float side;
in vec4 color;
uniform vec4 affine;
uniform vec2 size;
out vec4 v;
void main() {
  vec2 p = pos * affine.xy + affine.zw;
  vec2 d = dir * affine.xy;
  if (d != vec2(0.0)) {
    vec2 n = normalize(vec2(-d.y, d.x));
    p += n * side * 0.5;
  }
  gl_Position = vec4(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0, 0.0, 1.0);
  v = color;
}`;
const FRAGMENT = `#version 300 es
precision mediump float;
in vec4 v;
out vec4 o;
void main() {
  o = v;
}`;

export type Affine = { sx: number; sy: number; tx: number; ty: number };
export type TerritoryGl = {
  draw: (affine: Affine, width: number, height: number, dpr: number) => void;
  clear: () => void;
  destroy: () => void;
};

const compile = (gl: WebGL2RenderingContext, type: number, source: string): WebGLShader => {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("territories: no shader");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS))
    throw new Error(`territories: ${gl.getShaderInfoLog(shader) ?? "shader failed"}`);
  return shader;
};

const STRIDE = 9;

const mesh = (gl: WebGL2RenderingContext, program: WebGLProgram, data: number[]) => {
  const vao = gl.createVertexArray();
  const buffer = gl.createBuffer();
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(data), gl.STATIC_DRAW);
  const attribute = (name: string, count: number, offset: number) => {
    const at = gl.getAttribLocation(program, name);
    gl.enableVertexAttribArray(at);
    gl.vertexAttribPointer(at, count, gl.FLOAT, false, STRIDE * 4, offset * 4);
  };
  attribute("pos", 2, 0);
  attribute("dir", 2, 2);
  attribute("side", 1, 4);
  attribute("color", 4, 5);
  gl.bindVertexArray(null);
  return { vao, buffer, count: data.length / STRIDE };
};

export const createTerritoryGl = (
  canvas: HTMLCanvasElement,
  level: TerritoryLevel
): TerritoryGl | null => {
  const gl = canvas.getContext("webgl2", { premultipliedAlpha: true, antialias: true });
  if (!gl) return null;

  const grid = { size: level.hex_size, origin: level.origin };
  const { dx, dy } = hexCorners(grid);
  const fill: number[] = [];
  const line: number[] = [];
  const push = (out: number[], x: number, y: number, c: number[], dx = 0, dy = 0, side = 0) =>
    out.push(x, y, dx, dy, side, c[0], c[1], c[2], c[3]);
  for (const f of level.features) {
    const [r, g, b] = communityColor(f.community);
    const fc = [(r / 255) * FILL_ALPHA, (g / 255) * FILL_ALPHA, (b / 255) * FILL_ALPHA, FILL_ALPHA];
    const lc = [(r / 255) * LINE_ALPHA, (g / 255) * LINE_ALPHA, (b / 255) * LINE_ALPHA, LINE_ALPHA];
    const cells = new Set<number>();
    eachHexCell(f.hexes, (q, r) => cells.add(cellKey(q, r)));
    eachHexCell(f.hexes, (q, r) => {
      const [cx, cy] = hexCenter(grid, q, r);
      for (let k = 1; k < 5; k++) {
        push(fill, cx + dx[0], cy + dy[0], fc);
        push(fill, cx + dx[k], cy + dy[k], fc);
        push(fill, cx + dx[k + 1], cy + dy[k + 1], fc);
      }
      for (let k = 0; k < 6; k++) {
        if (cells.has(cellKey(q + HEX_ACROSS[k][0], r + HEX_ACROSS[k][1]))) continue;
        const k1 = (k + 1) % 6;
        const x0 = cx + dx[k];
        const y0 = cy + dy[k];
        const x1 = cx + dx[k1];
        const y1 = cy + dy[k1];
        const ex = x1 - x0;
        const ey = y1 - y0;
        push(line, x0, y0, lc, ex, ey, -1);
        push(line, x1, y1, lc, ex, ey, -1);
        push(line, x1, y1, lc, ex, ey, 1);
        push(line, x0, y0, lc, ex, ey, -1);
        push(line, x1, y1, lc, ex, ey, 1);
        push(line, x0, y0, lc, ex, ey, 1);
      }
    });
  }

  const program = gl.createProgram();
  if (!program) return null;
  gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS))
    throw new Error(`territories: ${gl.getProgramInfoLog(program) ?? "link failed"}`);
  const affineAt = gl.getUniformLocation(program, "affine");
  const sizeAt = gl.getUniformLocation(program, "size");
  const fills = mesh(gl, program, fill);
  const lines = mesh(gl, program, line);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);

  const fit = (width: number, height: number, dpr: number) => {
    const w = Math.round(width * dpr);
    const h = Math.round(height * dpr);
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    gl.viewport(0, 0, w, h);
  };
  return {
    draw: (a, width, height, dpr) => {
      fit(width, height, dpr);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.useProgram(program);
      gl.uniform4f(affineAt, a.sx, a.sy, a.tx, a.ty);
      gl.uniform2f(sizeAt, width, height);
      gl.bindVertexArray(fills.vao);
      gl.drawArrays(gl.TRIANGLES, 0, fills.count);
      gl.bindVertexArray(lines.vao);
      gl.drawArrays(gl.TRIANGLES, 0, lines.count);
      gl.bindVertexArray(null);
    },
    clear: () => {
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
    },
    destroy: () => {
      gl.deleteBuffer(fills.buffer);
      gl.deleteBuffer(lines.buffer);
      gl.deleteVertexArray(fills.vao);
      gl.deleteVertexArray(lines.vao);
      gl.deleteProgram(program);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    }
  };
};
