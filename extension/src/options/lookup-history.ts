const KEY = "_yomeru_lookup_history";
const MAX = 10;

export async function loadHistory(): Promise<string[]> {
  const res = await browser.storage.local.get(KEY);
  const list = (res[KEY] as unknown) as string[] | undefined;
  return Array.isArray(list) ? list.slice(0, MAX) : [];
}

export async function pushHistory(word: string): Promise<string[]> {
  const w = word.trim();
  if (!w) return loadHistory();
  const current = await loadHistory();
  const next = [w, ...current.filter((x) => x !== w)].slice(0, MAX);
  await browser.storage.local.set({ [KEY]: next });
  return next;
}

export async function clearHistory(): Promise<void> {
  await browser.storage.local.remove(KEY);
}
