import type * as SrsWasm from "../../_generated/srs-wasm/srs_wasm.js";
import type * as KanjiWasm from "../../_generated/kanjidic-wasm/kanjidic_wasm.js";
import type * as ExamplesWasm from "../../_generated/examples-wasm/examples_wasm.js";
import {
  putCard,
  getCard,
  getAllCards,
  getDueCards,
  getStagingCards,
  promoteCard,
  promoteAll,
  deleteCard,
  addLookupHistory,
} from "./idb";
import { getSettings, saveSettings } from "./settings";
import type { SrsCard, SrsSettings } from "../shared/types.ts";
import { mergeReview, applyIntervalScale, checkGraduation } from "./review-utils.ts";

type SrsEngine = InstanceType<typeof SrsWasm.SrsEngine>;
type KanjiDictionary = InstanceType<typeof KanjiWasm.KanjiDictionary>;
type ExamplesDict = InstanceType<typeof ExamplesWasm.ExamplesDict>;

let srs: SrsEngine | null = null;
let kanji: KanjiDictionary | null = null;
let examplesDict: ExamplesDict | null = null;
let examplesUnavailable = false;

async function initSrs(): Promise<void> {
  const jsUrl = browser.runtime.getURL("_generated/srs-wasm/srs_wasm.js");
  const binUrl = browser.runtime.getURL("_generated/srs-wasm/srs_wasm_bg.wasm");
  const mod = (await import(/* @vite-ignore */ jsUrl)) as typeof SrsWasm;
  await mod.default(binUrl);
  srs = new mod.SrsEngine();
}

async function ensureSrs(): Promise<void> {
  if (!srs) await initSrs();
}

async function initKanji(): Promise<void> {
  const jsUrl = browser.runtime.getURL("_generated/kanjidic-wasm/kanjidic_wasm.js");
  const binUrl = browser.runtime.getURL("_generated/kanjidic-wasm/kanjidic_wasm_bg.wasm");
  const mod = (await import(/* @vite-ignore */ jsUrl)) as typeof KanjiWasm;
  await mod.default(binUrl);
  const dataUrl = browser.runtime.getURL("data/kanjidic.bin");
  const buf = await fetch(dataUrl).then((r) => r.arrayBuffer());
  kanji = new mod.KanjiDictionary(new Uint8Array(buf));
}

async function ensureKanji(): Promise<void> {
  if (!kanji) await initKanji();
}

async function initExamples(): Promise<void> {
  const jsUrl = browser.runtime.getURL("_generated/examples-wasm/examples_wasm.js");
  const binUrl = browser.runtime.getURL("_generated/examples-wasm/examples_wasm_bg.wasm");
  const mod = (await import(/* @vite-ignore */ jsUrl)) as typeof ExamplesWasm;
  await mod.default(binUrl);
  const dataUrl = browser.runtime.getURL("data/examples.bin");
  const buf = await fetch(dataUrl).then((r) => r.arrayBuffer());
  examplesDict = new mod.ExamplesDict(new Uint8Array(buf));
}

async function ensureExamples(): Promise<void> {
  if (examplesDict || examplesUnavailable) return;
  try {
    await initExamples();
  } catch {
    examplesUnavailable = true;
  }
}

initSrs();
initKanji();

browser.runtime.onMessage.addListener(
  (msg: { type: string; payload?: unknown }) => {
    switch (msg.type) {
      case "ADD_WORD":
        return handleAddWord(
          msg.payload as { word: string; reading: string; meaning_en: string },
        );
      case "REVIEW_CARD":
        return handleReviewCard(
          msg.payload as { word: string; rating: number },
        );
      case "GET_DUE":
        return handleGetDue();
      case "GET_ALL_CARDS":
        return handleGetAllCards();
      case "DELETE_CARD":
        return handleDeleteCard(msg.payload as { word: string });
      case "LOG_LOOKUP":
        return handleLogLookup(
          msg.payload as { word: string; reading: string },
        );
      case "GET_SRS_WORDS":
        return handleGetSrsWords();
      case "GET_STAGING":
        return handleGetStaging();
      case "PROMOTE_CARD":
        return handlePromoteCard(msg.payload as { word: string });
      case "PROMOTE_ALL":
        return handlePromoteAll();
      case "GET_SETTINGS":
        return handleGetSettings();
      case "SAVE_SETTINGS":
        return handleSaveSettings(msg.payload as SrsSettings);
      case "GET_KANJI":
        return handleGetKanji(msg.payload as { word: string });
      case "GET_EXAMPLES":
        return handleGetExamples(msg.payload as { word: string });
      default:
        return Promise.resolve({ error: "Unknown message type" });
    }
  },
);

async function handleAddWord({
  word,
  reading,
  meaning_en,
  senses,
}: {
  word: string;
  reading: string;
  meaning_en: string;
  senses?: SrsCard["senses"];
}) {
  await ensureSrs();
  const existing = await getCard(word);
  if (existing) {
    return { success: true, existing: true };
  }
  const settings = await getSettings();
  if (settings.maxStagingSize > 0) {
    const stagingCount = (await getStagingCards()).length;
    if (stagingCount >= settings.maxStagingSize)
      return { success: false, reason: "staging_full" };
  }
  const base = srs!.new_card(word, reading, meaning_en ?? "", Date.now()) as SrsCard;
  const card: SrsCard = {
    ...base,
    senses: senses ?? [],
    status: "staging",
  };
  await putCard(card);
  return { success: true, existing: false };
}

async function handleReviewCard({
  word,
  rating,
}: {
  word: string;
  rating: number;
}) {
  await ensureSrs();
  const card = await getCard(word);
  if (!card) return { error: "Card not found" };
  const settings = await getSettings();
  const now_ms = Date.now();
  let updated = srs!.review_card(card, rating, now_ms) as SrsCard;
  updated = applyIntervalScale(updated, settings.intervalScale, now_ms);
  if (checkGraduation(updated.repetitions, settings.graduationReps)) {
    await deleteCard(word);
    return { success: true, graduated: true };
  }
  await putCard(mergeReview(card, updated));
  return { success: true, graduated: false };
}

async function handleGetDue() {
  const settings = await getSettings();
  const due = await getDueCards(Date.now());
  return { cards: due.slice(0, settings.maxSessionCards) };
}

async function handleGetStaging() {
  return { cards: await getStagingCards() };
}

async function handlePromoteCard({ word }: { word: string }) {
  await promoteCard(word);
  return { success: true };
}

async function handlePromoteAll() {
  await promoteAll();
  return { success: true };
}

async function handleGetSettings() {
  return getSettings();
}

async function handleSaveSettings(s: SrsSettings) {
  await saveSettings(s);
  return { success: true };
}

async function handleGetAllCards() {
  return { cards: await getAllCards() };
}

async function handleDeleteCard({ word }: { word: string }) {
  await deleteCard(word);
  return { success: true };
}

async function handleGetSrsWords(): Promise<{ words: string[] }> {
  const cards = await getAllCards();
  return { words: cards.map((c) => c.word) };
}

async function handleLogLookup({
  word,
  reading,
}: {
  word: string;
  reading: string;
}) {
  await addLookupHistory(word, reading);
  return { success: true };
}

async function handleGetKanji({ word }: { word: string }) {
  await ensureKanji();
  const entries = kanji!.lookup_many(word) as import("../shared/types.ts").KanjiEntry[];
  return { entries: entries ?? [] };
}

async function handleGetExamples({ word }: { word: string }) {
  await ensureExamples();
  if (!examplesDict) return { entries: [] };
  const entries = examplesDict.lookup(word, 5) as import("../shared/types.ts").ExampleEntry[];
  return { entries: entries ?? [] };
}
