/**
 * Message handlers: one function per message type the popup and content script
 * can send. The routing table itself lives in `background.ts`.
 */

import {
  putCard,
  putCards,
  getCard,
  getCardsOf,
  getAllCards,
  getDueCards,
  getStagingCards,
  promoteCard,
  bumpPriority,
  resetCard,
  getWordStatus,
  deleteCard,
  addLookupHistory,
  putCardLogged,
} from "./idb";
import { getSettings, saveSettings } from "./settings";
import { importCards } from "./cards-backup";
import { getSrs, getKanji, getExamples, getJmdict, getGrammar } from "./engines";
import { bumpDbVersion } from "./sync";
import type {
  CardDirection,
  CardKey,
  CardKind,
  ExampleEntry,
  GrammarPopupMatch,
  KanjiEntry,
  PartInfo,
  PartSearch,
  SrsCard,
  SrsSettings,
  WordEntry,
  WordStatus,
} from "../shared/types.ts";
import { cardId, isWordCard, KIND_DIRECTIONS, MS_PER_DAY, wordKey } from "../shared/types.ts";
import {
  mergeReview,
  applyIntervalScale,
  checkGraduation,
  type SrsSchedFields,
} from "./review-utils.ts";

// The Rust SrsCard the WASM works with is the FSRS scheduling subset plus
// `sequence` + `added_ms` — no id/kind/item/direction/status. Its `sequence`
// is unused by the scheduler: 0 goes in, and it is dropped on the way out. We
// never trust the JS-typed shape after a WASM round-trip; mergeReview
// reattaches the JS-only fields from the original card.
type WasmCardShape = SrsSchedFields & Pick<SrsCard, "added_ms"> & { sequence?: number };

/** Stages the cards of `key`, one per direction of its kind, unless it has
 * some already. */
async function addItem(key: CardKey) {
  if ((await getCardsOf(key)).length > 0) {
    return { success: true, existing: true };
  }
  const srs = await getSrs();
  const { sequence: _unused, ...base } = srs.new_card(0, Date.now()) as WasmCardShape;
  await putCards(
    KIND_DIRECTIONS[key.kind].map((direction) => ({
      ...base,
      id: cardId(key, direction),
      kind: key.kind,
      item: key.item,
      direction,
      status: "staging" as const,
      priority: 0,
    })),
  );
  await bumpDbVersion();
  return { success: true, existing: false };
}

/** Stages the word with JMdict `ent_seq` `sequence`, both directions. */
export async function handleAddWord({ sequence }: { sequence: number }) {
  return addItem(wordKey(sequence));
}

/** Stages the card of the grammar point numbered `seq`, which the popup got
 * from `popup_lookup`. Its status then comes from `GET_WORD_STATUS`. */
export async function handleAddGrammar({ seq }: { seq: number }) {
  if (!Number.isInteger(seq) || seq <= 0) return { error: "not a grammar point" };
  return addItem({ kind: "grammar", item: String(seq) });
}

export async function handleReviewCard({
  kind,
  item,
  direction,
  rating,
}: CardKey & {
  direction: CardDirection;
  rating: number;
}) {
  const srs = await getSrs();
  const card = await getCard({ kind, item }, direction);
  if (!card) return { error: "Card not found" };
  const settings = await getSettings();
  const now_ms = Date.now();
  const wasmOut = srs.review_card({ ...card, sequence: 0 }, rating, now_ms) as WasmCardShape;
  const scaled = applyIntervalScale(wasmOut, settings.intervalScale, now_ms);
  const intervalDays = (scaled.due_ms - now_ms) / MS_PER_DAY;
  const reviewed = mergeReview(card, scaled);
  if (rating === 1) reviewed.again_count = (card.again_count ?? 0) + 1;
  // The card and its log row are written together, so the log can neither
  // record a review that didn't apply nor miss one that did.
  const row = { card_id: card.id, rating, kind: "review" as const, reviewed_ms: now_ms };
  if (checkGraduation(intervalDays, settings.graduationIntervalDays)) {
    await putCardLogged({ ...reviewed, status: "graduated" }, row);
    await bumpDbVersion();
    return { success: true, graduated: true };
  }
  await putCardLogged(reviewed, row);
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

export async function handlePromoteCard(key: CardKey) {
  await promoteCard(key);
  await bumpDbVersion();
  return { success: true };
}

export async function handleBumpPriority(key: CardKey) {
  await bumpPriority(key);
  await bumpDbVersion();
  return getWordStatus(key);
}

export async function handleResetCard(key: CardKey) {
  await resetCard(key, Date.now());
  await bumpDbVersion();
  return getWordStatus(key);
}

/** SRS standing of each word or grammar point the hover popup is showing,
 * keyed by item. */
export async function handleGetWordStatus({
  kind,
  items,
}: {
  kind: CardKind;
  items: string[];
}): Promise<{ statuses: Record<string, WordStatus> }> {
  const statuses: Record<string, WordStatus> = {};
  for (const item of items) statuses[item] = await getWordStatus({ kind, item });
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

export async function handleDeleteCard(key: CardKey) {
  await deleteCard(key);
  await bumpDbVersion();
  return { success: true };
}

// The highlighter matches page text by JMdict entry, so it only needs the
// ent_seq of each word card — no surface strings. Kanji and grammar cards
// are left out: they are no entry.
export async function handleGetSrsSequences(): Promise<{ sequences: number[] }> {
  const cards = await getAllCards();
  const words = cards.filter(isWordCard);
  return { sequences: [...new Set(words.map((c) => Number(c.item)))] };
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

export async function handleGetKanjiParts() {
  const kanji = await getKanji();
  return { parts: (kanji.parts() as PartInfo[]) ?? [] };
}

export async function handleKanjiByParts({
  parts,
  max,
}: {
  parts: string;
  max: number;
}) {
  const kanji = await getKanji();
  return kanji.withParts(parts, max) as PartSearch;
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

/** The extension pages' `DictClient::grammar_by_seq`: full entries aligned
 * with `sequences`, `null` for a point that left the dictionary. */
export async function handleGrammarBySequence({ sequences }: { sequences: number[] }) {
  const dict = await getGrammar();
  if (!dict) return { results: sequences.map(() => null) };
  return { results: (dict.by_seq(Uint32Array.from(sequences)) as unknown[]) ?? [] };
}

export async function handleLookupPrefix({ text, max }: { text: string; max: number }) {
  const jmdict = await getJmdict();
  const results = (jmdict.lookup_prefix(text, max) as WordEntry[]) ?? [];
  return { results };
}

/** Longest grammar pattern hovered `text` starts with. */
const GRAMMAR_MAX_CHARS = 24;
/** Examples per grammar point in the popup. */
const GRAMMAR_EXAMPLES = 2;

/**
 * The grammar point `text` starts with, for the hover popup. `match: null`
 * when there is none.
 */
export async function handleGrammarLookup({
  text,
}: {
  text: string;
}): Promise<{ match: GrammarPopupMatch | null }> {
  const dict = await getGrammar();
  if (!dict || !text) return { match: null };
  const match = dict.popup_lookup(text, GRAMMAR_MAX_CHARS, GRAMMAR_EXAMPLES) as GrammarPopupMatch | null;
  return { match: match ?? null };
}

/** What the Lookup page's search shows for `text`, at most `max` points. */
export async function handleGrammarSearch({
  text,
  max,
}: {
  text: string;
  max: number;
}): Promise<{ entries: unknown[] }> {
  const dict = await getGrammar();
  if (!dict || !text) return { entries: [] };
  return { entries: (dict.search(text, max) as unknown[]) ?? [] };
}

export async function handleImportCards({ cards }: { cards: unknown }) {
  const result = await importCards(cards);
  if (result.added > 0) await bumpDbVersion();
  return result;
}
