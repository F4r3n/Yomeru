import type { WordEntry } from "../shared/types.ts";

const cache = new Map<string, WordEntry | null>();

/** Look up a word in the JMdict via the background. Cached per session. Returns null if not in the dictionary. */
export async function lookupWord(word: string): Promise<WordEntry | null> {
  if (cache.has(word)) return cache.get(word)!;
  const res = await browser.runtime.sendMessage({
    type: "LOOKUP_WORD",
    payload: { word },
  }) as { entries: WordEntry[] };
  const entry = res.entries?.[0] ?? null;
  cache.set(word, entry);
  return entry;
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
