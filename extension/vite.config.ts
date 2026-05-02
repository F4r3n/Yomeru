import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Builds background (ESM) and options page (ESM + CSS injected).
export default defineConfig({
  plugins: [svelte({ emitCss: false })],
  build: {
    outDir: "dist",
    emptyOutDir: false,
    rollupOptions: {
      input: {
        background: resolve(__dirname, "src/background/background.ts"),
        options: resolve(__dirname, "src/options/main.ts"),
      },
      output: {
        entryFileNames: "[name].js",
        chunkFileNames: "_chunks/[name]-[hash].js",
        format: "es",
      },
    },
    minify: false,
  },
});
