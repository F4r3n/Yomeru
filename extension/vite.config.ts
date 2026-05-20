import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";
import { execSync } from "child_process";

const commitHash = (() => {
  try { return execSync("git rev-parse --short HEAD").toString().trim(); }
  catch { return "unknown"; }
})();

const __dirname = dirname(fileURLToPath(import.meta.url));

// Builds background (ESM) and options page (ESM + CSS injected).
export default defineConfig({
  plugins: [svelte({ emitCss: false })],
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
