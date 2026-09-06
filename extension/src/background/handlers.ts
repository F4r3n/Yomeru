/**
 * Message handlers: one function per message type the popup and content script
 * can send. The routing table itself lives in `background.ts`.
 */

import {
  putCard,
  putCards,
  getCard,
  getCardsBySequence,
  getAllCards,
  getDueCards,
  getStagingCards,
  promoteCard,
  promoteAll,
  deleteCard,
  addLookupHistory,
} from "./idb";
import { getSettings, saveSettings } from "./settings";
import { importCards } from "./cards-backup";
import { getSrs, getKanji, getExamples, getJmdict } from "./engines";
import { bumpDbVersion } from "./sync";
import type {
  CardDirection,
  ExampleEntry,
  KanjiEntry,
  SrsCard,
  SrsSettings,
  WordEntry,
} from "../shared/types.ts";
import { cardId, MS_PER_DAY } from "../shared/types.ts";
import {
  mergeReview,
  applyIntervalScale,
  checkGraduation,
  type SrsSchedFields,
} from "./review-utils.ts";

// The Rust SrsCard the WASM works with is the FSRS scheduling subset plus
// `sequence` + `added_ms` — no id/direction/status. We never trust the
// JS-typed shape after a WASM round-trip; mergeReview reattaches the JS-only
// fields from the original card.
type WasmCardShape = SrsSchedFields & Pick<SrsCard, "sequence" | "added_ms">;

export async function handleAddWord({ sequence }: { sequence: number }) {
  const srs = await getSrs();
  const siblings = await getCardsBySequence(sequence);
  if (siblings.length > 0) {
    return { success: true, existing: true };
  }
  const now = Date.now();
  const base = srs.new_card(sequence, now) as WasmCardShape;
  const recognition: SrsCard = {
    ...base,
    id: cardId(sequence, "recognition"),
    sequence,
    direction: "recognition",
    status: "staging",
    priority: 0,
  };
  const recall: SrsCard = {
    ...base,
    id: cardId(sequence, "recall"),
    sequence,
    direction: "recall",
    status: "staging",
    priority: 0,
  };
  await putCards([recognition, recall]);
  await bumpDbVersion();
  return { success: true, existing: false };
}

export async function handleReviewCard({
  sequence,
  direction,
  rating,
}: {
  sequence: number;
  direction: CardDirection;
  rating: number;
}) {
  const srs = await getSrs();
  const card = await getCard(sequence, direction);
  if (!card) return { error: "Card not found" };
  const settings = await getSettings();
  const now_ms = Date.now();
  const wasmOut = srs.review_card(card, rating, now_ms) as WasmCardShape;
  const scaled = applyIntervalScale(wasmOut, settings.intervalScale, now_ms);
  const intervalDays = (scaled.due_ms - now_ms) / MS_PER_DAY;
  if (checkGraduation(intervalDays, settings.graduationIntervalDays)) {
    await putCard({ ...mergeReview(card, scaled), status: "graduated" });
    await bumpDbVersion();
    return { success: true, graduated: true };
  }
  await putCard(mergeReview(card, scaled));
  await bumpDbVersion();
  return { success: true, graduated: false };
}

export async function handleGetDue() {
  const settings = await getSettings();
  const due = await getDueCards(Date.now());
  return { cards: due.slice(0, settings.maxSessionCards) };
}

export async function handleGetStaging() {
  return { cards: await getStagingCards() };
}

export async function handlePromoteCard({ sequence }: { sequence: number }) {
  await promoteCard(sequence);
  await bumpDbVersion();
  return { success: true };
}

export async function handlePromoteAll() {
  await promoteAll();
  await bumpDbVersion();
  return { success: true };
}

export async function handlePromoteBatch() {
  const settings = await getSettings();
  const staging = (await getStagingCards()).sort(
    (a, b) => a.added_ms - b.added_ms,
  );
  const stagingSeqs: number[] = [];
  const seen = new Set<number>();
  for (const c of staging) {
    if (!seen.has(c.sequence)) {
      seen.add(c.sequence);
      stagingSeqs.push(c.sequence);
    }
  }
  const n = Math.min(stagingSeqs.length, settings.maxSessionCards);
  for (let i = 0; i < n; i++) {
    await promoteCard(stagingSeqs[i]);
  }
  if (n > 0) await bumpDbVersion();
  const due = await getDueCards(Date.now());
  return {
    cards: due.slice(0, settings.maxSessionCards),
    stagingCount: stagingSeqs.length - n,
  };
}

export async function handleGetSettings() {
  return getSettings();
}

export async function handleSaveSettings(s: SrsSettings) {
  await saveSettings(s);
  return { success: true };
}

export async function handleGetAllCards() {
  return { cards: await getAllCards() };
}

export async function handleDeleteCard({ sequence }: { sequence: number }) {
  await deleteCard(sequence);
  await bumpDbVersion();
  return { success: true };
}

// Highlighting matches surface strings against page text, but cards now key on
// `sequence`. Resolve each active card's sequence to all of its kanji + reading
// surface forms so the content-script highlighter can keep underlining them.
export async function handleGetSrsWords(): Promise<{ words: string[] }> {
  const jmdict = await getJmdict();
  const cards = await getAllCards();
  const seqs = [...new Set(cards.map((c) => c.sequence))];
  const entries = jmdict.lookup_by_sequence(seqs) as (WordEntry | null)[];
  const words = new Set<string>();
  for (const e of entries) {
    if (!e) continue;
    for (const k of e.kanji_forms) words.add(k.text);
    for (const r of e.reading_forms) words.add(r.text);
  }
  return { words: [...words] };
}

export async function handleLogLookup({
  word,
  reading,
}: {
  word: string;
  reading: string;
}) {
  await addLookupHistory(word, reading);
  return { success: true };
}

export async function handleGetKanji({ word }: { word: string }) {
  const kanji = await getKanji();
  const entries = kanji.lookup_many(word) as KanjiEntry[];
  return { entries: entries ?? [] };
}

export async function handleGetExamples({ word }: { word: string }) {
  const examplesDict = await getExamples();
  if (!examplesDict) return { entries: [] };
  const entries = examplesDict.lookup(word, 5) as ExampleEntry[];
  return { entries: entries ?? [] };
}

export async function handleLookupWord({ word }: { word: string }) {
  const jmdict = await getJmdict();
  const entries = jmdict.lookup(word) as WordEntry[];
  return { entries: entries ?? [] };
}

export async function handleLookupMany({ words }: { words: string[] }) {
  const jmdict = await getJmdict();
  // The Rust shared crate's lookup_many returns Vec<Vec<WordEntry>> —
  // one entry list per input word, aligned by index. Mirror that shape.
  const results: WordEntry[][] = words.map(
    (w) => (jmdict.lookup(w) as WordEntry[]) ?? [],
  );
  return { results };
}

export async function handleLookupBySequence({ sequences }: { sequences: number[] }) {
  const jmdict = await getJmdict();
  const results = (jmdict.lookup_by_sequence(
    sequences,
  ) as (WordEntry | null)[]) ?? [];
  return { results };
}

export async function handleLookupPrefix({ text, max }: { text: string; max: number }) {
  const jmdict = await getJmdict();
  const results = (jmdict.lookup_prefix(text, max) as WordEntry[]) ?? [];
  return { results };
}

export async function handleImportCards({ cards }: { cards: unknown }) {
  const result = await importCards(cards);
  if (result.added > 0) await bumpDbVersion();
  return result;
}
