const DB_VERSION_KEY = "_yomeru_db_v";
const DEBOUNCE_MS = 150;

/**
 * Calls `cb` immediately and again whenever the cards DB version bumps in
 * storage.local (debounced 150ms). Returns the cleanup fn, intended to be
 * returned from a Svelte `$effect`.
 */
export function watchCardsDb(cb: () => void): () => void {
  cb();
  let pending: ReturnType<typeof setTimeout> | null = null;
  const handler = (
    changes: Record<string, browser.storage.StorageChange>,
    area: string,
  ) => {
    if (area !== "local" || !(DB_VERSION_KEY in changes)) return;
    if (pending) clearTimeout(pending);
    pending = setTimeout(() => { pending = null; cb(); }, DEBOUNCE_MS);
  };
  browser.storage.onChanged.addListener(handler);
  return () => {
    if (pending) clearTimeout(pending);
    browser.storage.onChanged.removeListener(handler);
  };
}
