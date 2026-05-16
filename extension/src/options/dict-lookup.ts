import type { WordEntry } from "../shared/types.ts";

const cache = new Map<string, WordEntry[]>();

/** Look up all dictionary entries for a word (homophones return multiple). Cached per session. */
export async function lookupAllEntries(word: string): Promise<WordEntry[]> {
  if (cache.has(word)) return cache.get(word)!;
  const res = await browser.runtime.sendMessage({
    type: "LOOKUP_WORD",
    payload: { word },
  }) as { entries: WordEntry[] };
  const entries = res.entries ?? [];
  cache.set(word, entries);
  return entries;
}

/** First dictionary entry for a word, or null. Convenience for card display where one canonical entry is enough. */
export async function lookupWord(word: string): Promise<WordEntry | null> {
  const entries = await lookupAllEntries(word);
  return entries[0] ?? null;
}

/** Look up multiple words in parallel. Returned in input order. */
export async function lookupWords(words: string[]): Promise<(WordEntry | null)[]> {
  return Promise.all(words.map(lookupWord));
}

/** Look up `words` and return a `word -> entry` map. Missing entries map to null. */
export async function buildEntryMap(words: string[]): Promise<Record<string, WordEntry | null>> {
  const entries = await lookupWords(words);
  const map: Record<string, WordEntry | null> = {};
  words.forEach((w, i) => { map[w] = entries[i]; });
  return map;
}

export function readingOf(entry: WordEntry | null): string {
  return entry?.reading_forms[0]?.text ?? "";
}

export function meaningOf(entry: WordEntry | null): string {
  return entry?.senses[0]?.glosses[0]?.text ?? "";
}
