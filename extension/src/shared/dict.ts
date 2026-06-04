import type { WordEntry } from "./types.ts";

/** Lower is more common. `Infinity` means no priority tags at all. Mirrors
 *  `priority_score` in app/shared/src/dict.rs. */
export function priorityScore(tags: string[] = []): number {
  let best = Infinity;
  for (const t of tags) {
    if (t.startsWith("nf")) {
      const n = parseInt(t.slice(2), 10);
      if (!Number.isNaN(n)) best = Math.min(best, n);
    } else if (t === "news1" || t === "ichi1" || t === "spec1" || t === "gai1") {
      best = Math.min(best, 1);
    } else if (t === "news2" || t === "ichi2" || t === "spec2" || t === "gai2") {
      best = Math.min(best, 24);
    }
  }
  return best;
}

/** Headword to display as the card title — prefers the kana reading when the
 *  word is usually written in kana. Mirrors `preferred_headword` in dict.rs. */
export function preferredHeadword(e: WordEntry): string {
  const kanji = e.kanji_forms[0];
  const reading = e.reading_forms[0];
  if (!kanji) return reading?.text ?? "";
  if (!reading) return kanji.text;

  const usuallyKana = e.senses.some((s) => (s.misc ?? []).includes("uk"));
  const onlyRareKanji = e.kanji_forms.every((k) =>
    (k.info ?? []).some((i) => i === "rK" || i === "sK"),
  );
  if (usuallyKana || onlyRareKanji) return reading.text;

  if (priorityScore(reading.priorities) < priorityScore(kanji.priorities)) {
    return reading.text;
  }
  return kanji.text;
}

/** Human-readable frequency band from the entry's priority tags, or `null`
 *  when the entry carries none. Mirrors `frequency_label` in dict.rs. */
export function frequencyLabel(e: WordEntry): string | null {
  const scores = [
    ...e.kanji_forms.map((k) => priorityScore(k.priorities)),
    ...e.reading_forms.map((r) => priorityScore(r.priorities)),
  ];
  const best = scores.length ? Math.min(...scores) : Infinity;
  if (!Number.isFinite(best)) return null;
  if (best <= 2) return "Top 1k";
  if (best <= 10) return "Top 5k";
  if (best <= 24) return "Common";
  return "Uncommon";
}
