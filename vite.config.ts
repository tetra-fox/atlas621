import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig, type Plugin } from "vite";

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
  esbuild: command === "build" ? { drop: ["console", "debugger"] } : {}
}));
