module.exports = {
  // Global options: they apply to every web-ext command, so they can't live
  // under `run` (web-ext rejects them there as unknown options).
  sourceDir: ".",
  // The dev profile sits inside sourceDir; without this, Firefox's own writes
  // to it would trigger extension reloads. Sources are ignored too: only the
  // built output (dist/, _generated/) should cause a reload.
  ignoreFiles: [
    "dev-profile",
    "dev-profile/**",
    "src",
    "src/**",
    "tsconfig.json",
    "vite.config.ts",
    "vite.content.config.ts",
    "package.json",
    "package-lock.json",
    "amo-metadata.json",
  ],
  run: {
    firefoxProfile: "./dev-profile",
    profileCreateIfMissing: true,
    keepProfileChanges: true,
  },
};
