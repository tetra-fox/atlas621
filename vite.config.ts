import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

const jsProfiling = (): Plugin => ({
  name: "js-profiling-header",
  configureServer(server) {
    server.middlewares.use((_req, res, next) => {
      res.setHeader("Document-Policy", "js-profiling");
      next();
    });
  }
});

export default defineConfig(({ command }) => ({
  plugins: [tailwindcss(), sveltekit(), ...(command === "serve" ? [jsProfiling()] : [])],
  resolve: {
    alias: [{ find: /^gl-bench$/, replacement: "gl-bench/dist/gl-bench.module.js" }]
  },
  build: {
    target: "es2025"
  },
  ...(command === "build" ? { esbuild: { drop: ["console", "debugger"] } } : {}),
  test: {
    // unit tests sit next to what they cover; anything needing a real canvas or layout is not
    // covered here, node has neither
    include: ["src/**/*.test.ts"]
  }
}));
