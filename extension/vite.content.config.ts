import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Produces a single IIFE bundle at content/content.js.
// IIFE format avoids runtime module loading issues in content scripts.
// The WASM dynamic import() uses /* @vite-ignore */ so Vite leaves it as-is.
export default defineConfig({
  plugins: [svelte({ emitCss: false })],
  build: {
    outDir: resolve(__dirname, "content"),
    emptyOutDir: false,
    rollupOptions: {
      input: resolve(__dirname, "src/content/content.ts"),
      output: {
        format: "iife",
        name: "_JpReader",
        entryFileNames: "content.js",
      },
    },
    minify: false,
  },
});
