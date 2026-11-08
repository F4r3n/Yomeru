// Diagnostic logging that only exists in development builds. Vite replaces
// `import.meta.env.MODE` with "production" for a plain `vite build`, so release
// builds get a no-op; `--mode development` builds (`npm run build:dev`, the
// `dev:*` watch scripts) and vitest keep it. (`DEV` would not work here:
// `vite build` reports DEV=false whatever the mode.)
// Real failures should still use console.warn / console.error directly.
export const debug: (...args: unknown[]) => void = import.meta.env.MODE !== "production"
  ? console.debug.bind(console, "[yomeru]")
  : () => {};
