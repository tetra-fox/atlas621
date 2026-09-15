import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig(({ command }) => ({
  plugins: [tailwindcss(), sveltekit()],
  build: {
    target: "es2025"
  },
  esbuild: command === "build" ? { drop: ["console", "debugger"] } : {}
}));
