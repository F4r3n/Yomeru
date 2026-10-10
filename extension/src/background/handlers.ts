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
  bumpPriority,
  resetCard,
  getWordStatus,
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
  WordStatus,
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
  const reviewed = mergeReview(card, scaled);
  if (rating === 1) reviewed.again_count = (card.again_count ?? 0) + 1;
  if (checkGraduation(intervalDays, settings.graduationIntervalDays)) {
    await putCard({ ...reviewed, status: "graduated" });
    await bumpDbVersion();
    return { success: true, graduated: true };
  }
  await putCard(reviewed);
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

export async function handleBumpPriority({ sequence }: { sequence: number }) {
  await bumpPriority(sequence);
  await bumpDbVersion();
  return getWordStatus(sequence);
}

export async function handleResetCard({ sequence }: { sequence: number }) {
  await resetCard(sequence, Date.now());
  await bumpDbVersion();
  return getWordStatus(sequence);
}

/** SRS standing of each entry the hover popup is showing, keyed by sequence. */
export async function handleGetWordStatus({
  sequences,
}: {
  sequences: number[];
}): Promise<{ statuses: Record<number, WordStatus> }> {
  const statuses: Record<number, WordStatus> = {};
  for (const seq of sequences) statuses[seq] = await getWordStatus(seq);
  return { statuses };
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

// The highlighter matches page text by JMdict entry, so it only needs the
// sequence each card is keyed on — no surface strings. Kanji cards are left
// out: their `sequence` is a code point, not an entry.
export async function handleGetSrsSequences(): Promise<{ sequences: number[] }> {
  const cards = await getAllCards();
  const words = cards.filter((c) => c.direction !== "kanji");
  return { sequences: [...new Set(words.map((c) => c.sequence))] };
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
