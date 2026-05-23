import { defineConfig } from "vite";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";
import { execSync } from "child_process";

const commitHash = (() => {
  try { return execSync("git rev-parse --short HEAD").toString().trim(); }
  catch { return "unknown"; }
})();

const __dirname = dirname(fileURLToPath(import.meta.url));

// Builds the background service worker (ESM).
// The options popup loads the pre-built Dioxus WASM bundle directly via
// options-dx-loader.js — no Vite processing needed for the options entry.
export default defineConfig({
  plugins: [],
  define: {
    __VERSION__: JSON.stringify(process.env.npm_package_version ?? "0.0.0"),
    __COMMIT__: JSON.stringify(commitHash),
    __BUILD_DATE__: JSON.stringify(new Date().toISOString()),
  },
  build: {
    outDir: "dist",
    emptyOutDir: false,
    rollupOptions: {
      input: {
        background: resolve(__dirname, "src/background/background.ts"),
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
