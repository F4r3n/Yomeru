import type * as SrsWasm from "../../_generated/srs-wasm/srs_wasm.js";
import type * as KanjiWasm from "../../_generated/kanjidic-wasm/kanjidic_wasm.js";
import type * as ExamplesWasm from "../../_generated/examples-wasm/examples_wasm.js";
import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";
import {
  putCard,
  getCard,
  getCardsByWord,
  getAllCards,
  getDueCards,
  getStagingCards,
  promoteCard,
  promoteAll,
  deleteCard,
  deleteCardById,
  addLookupHistory,
} from "./idb";
import { getSettings, saveSettings } from "./settings";
import { importCards, syncCardsBackup, writeCardsBackup } from "./cards-backup";
import type { CardDirection, SrsCard, SrsSettings } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";
import { mergeReview, applyIntervalScale, checkGraduation } from "./review-utils.ts";

type SrsEngine = InstanceType<typeof SrsWasm.SrsEngine>;
type KanjiDictionary = InstanceType<typeof KanjiWasm.KanjiDictionary>;
type ExamplesDict = InstanceType<typeof ExamplesWasm.ExamplesDict>;
type JmDictDictionary = InstanceType<typeof JmDictWasm.Dictionary>;

let srs: SrsEngine | null = null;
let kanji: KanjiDictionary | null = null;
let examplesDict: ExamplesDict | null = null;
let examplesUnavailable = false;
let jmdict: JmDictDictionary | null = null;

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

async function initJmdict(): Promise<void> {
  const jsUrl = browser.runtime.getURL("_generated/jmdict-wasm/jmdict_wasm.js");
  const binUrl = browser.runtime.getURL("_generated/jmdict-wasm/jmdict_wasm_bg.wasm");
  const mod = (await import(/* @vite-ignore */ jsUrl)) as typeof JmDictWasm;
  await mod.default(binUrl);
  const dataUrl = browser.runtime.getURL("data/jmdict.bin");
  const buf = await fetch(dataUrl).then((r) => r.arrayBuffer());
  jmdict = new mod.Dictionary(new Uint8Array(buf));
}

async function ensureJmdict(): Promise<void> {
  if (!jmdict) await initJmdict();
}

initSrs().catch((e) => console.error("[yomeru] initSrs failed:", e));
initKanji().catch((e) => console.error("[yomeru] initKanji failed:", e));

async function bumpDbVersion(): Promise<void> {
  await Promise.all([
    browser.storage.local.set({ _yomeru_db_v: Date.now() }),
    writeCardsBackup(),
  ]);
}

const storageReady = syncCardsBackup().catch((e) => {
  console.error("[yomeru] syncCardsBackup failed:", e);
});

function syncIcon(enabled: boolean) {
  browser.action.setIcon({
    path: enabled ? "icons/icon.svg" : "icons/icon-disabled.svg",
  });
}

browser.storage.local.get("enabled").then((res) => {
  const enabled = (res as { enabled?: boolean }).enabled ?? true;
  syncIcon(enabled);
});

browser.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && "enabled" in changes) {
    syncIcon(changes.enabled.newValue ?? true);
  }
});

function dispatch(msg: { type: string; payload?: unknown }): Promise<unknown> {
  switch (msg.type) {
    case "ADD_WORD":
      return handleAddWord(msg.payload as { word: string });
    case "REVIEW_CARD":
      return handleReviewCard(
        msg.payload as { word: string; direction: CardDirection; rating: number },
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
    case "PROMOTE_BATCH":
      return handlePromoteBatch();
    case "GET_SETTINGS":
      return handleGetSettings();
    case "SAVE_SETTINGS":
      return handleSaveSettings(msg.payload as SrsSettings);
    case "GET_KANJI":
      return handleGetKanji(msg.payload as { word: string });
    case "GET_EXAMPLES":
      return handleGetExamples(msg.payload as { word: string });
    case "LOOKUP_WORD":
      return handleLookupWord(msg.payload as { word: string });
    case "IMPORT_CARDS":
      return handleImportCards(msg.payload as { cards: unknown });
    default:
      return Promise.resolve({ error: "Unknown message type" });
  }
}

async function handleImportCards({ cards }: { cards: unknown }) {
  const result = await importCards(cards);
  if (result.added > 0) await bumpDbVersion();
  return result;
}

browser.runtime.onMessage.addListener(
  (msg: { type: string; payload?: unknown }) => storageReady.then(() => dispatch(msg)),
);

async function handleAddWord({ word }: { word: string }) {
  await ensureSrs();
  const siblings = await getCardsByWord(word);
  if (siblings.length > 0) {
    return { success: true, existing: true };
  }
  const now = Date.now();
  const base = srs!.new_card(word, now) as Omit<SrsCard, "id" | "direction" | "status">;
  const recognition: SrsCard = {
    ...base,
    id: cardId(word, "recognition"),
    word,
    direction: "recognition",
    status: "staging",
  };
  const recall: SrsCard = {
    ...base,
    id: cardId(word, "recall"),
    word,
    direction: "recall",
    status: "staging",
  };
  await putCard(recognition);
  await putCard(recall);
  await bumpDbVersion();
  return { success: true, existing: false };
}

async function handleReviewCard({
  word,
  direction,
  rating,
}: {
  word: string;
  direction: CardDirection;
  rating: number;
}) {
  await ensureSrs();
  const card = await getCard(word, direction);
  if (!card) return { error: "Card not found" };
  const settings = await getSettings();
  const now_ms = Date.now();
  let updated = srs!.review_card(card, rating, now_ms) as SrsCard;
  updated = applyIntervalScale(updated, settings.intervalScale, now_ms);
  if (checkGraduation(updated.repetitions, settings.graduationReps)) {
    await deleteCardById(cardId(word, direction));
    await bumpDbVersion();
    return { success: true, graduated: true };
  }
  await putCard(mergeReview(card, updated));
  await bumpDbVersion();
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
  await bumpDbVersion();
  return { success: true };
}

async function handlePromoteAll() {
  await promoteAll();
  await bumpDbVersion();
  return { success: true };
}

async function handlePromoteBatch() {
  const settings = await getSettings();
  const staging = (await getStagingCards()).sort((a, b) => a.added_ms - b.added_ms);
  const stagingWords: string[] = [];
  const seen = new Set<string>();
  for (const c of staging) {
    if (!seen.has(c.word)) {
      seen.add(c.word);
      stagingWords.push(c.word);
    }
  }
  const n = Math.min(stagingWords.length, settings.maxSessionCards);
  for (let i = 0; i < n; i++) {
    await promoteCard(stagingWords[i]);
  }
  if (n > 0) await bumpDbVersion();
  const due = await getDueCards(Date.now());
  return {
    cards: due.slice(0, settings.maxSessionCards),
    stagingCount: stagingWords.length - n,
  };
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
  await bumpDbVersion();
  return { success: true };
}

async function handleGetSrsWords(): Promise<{ words: string[] }> {
  const cards = await getAllCards();
  return { words: [...new Set(cards.map((c) => c.word))] };
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

async function handleLookupWord({ word }: { word: string }) {
  await ensureJmdict();
  const entries = jmdict!.lookup(word) as import("../shared/types.ts").WordEntry[];
  return { entries: entries ?? [] };
}
