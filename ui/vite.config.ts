/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  base: "/",
  build: { outDir: "dist" },
  // Use the `browser` entry points in `package.json` files. Required for
  // Vitest component tests (which run in Node) and correct for this
  // client-only SPA in dev/build as well.
  resolve: { conditions: ["browser"] },
  test: {
    environment: "jsdom",
    include: ["test/**/*.test.ts"],
    passWithNoTests: true,
    setupFiles: ["test/setup.ts"],
  },
});
