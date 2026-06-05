import { FreqKind, KanjiInf, Misc } from "./types.ts";
import type { Freq, WordEntry } from "./types.ts";

/** Lower is more common. `Infinity` means no priority tags at all. Mirrors
 *  `priority_score` in app/shared/src/dict.rs. */
export function priorityScore(tags: Freq[] = []): number {
  let best = Infinity;
  for (const t of tags) {
    if (t.kind === FreqKind.Nf) {
      // nf01–nf48: the value is the band directly.
      best = Math.min(best, t.value);
    } else {
      // news/ichi/spec/gai: tier-1 (value 1) → 1, tier-2 (value 2) → 24.
      best = Math.min(best, t.value === 1 ? 1 : 24);
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

  const usuallyKana = e.senses.some((s) => (s.misc ?? []).includes(Misc.UsuallyKana));
  const onlyRareKanji = e.kanji_forms.every((k) =>
    (k.info ?? []).some((i) => i === KanjiInf.RareKanji || i === KanjiInf.SearchOnlyKanji),
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
