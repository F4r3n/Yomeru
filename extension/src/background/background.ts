import type * as SrsWasm from "../../_generated/srs-wasm/srs_wasm.js";
import {
  putCard,
  getCard,
  getAllCards,
  getDueCards,
  deleteCard,
  addLookupHistory,
} from "./idb";
import type { SrsCard } from "../shared/types.ts";

type SrsEngine = InstanceType<typeof SrsWasm.SrsEngine>;

let srs: SrsEngine | null = null;

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

initSrs();

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
      default:
        return Promise.resolve({ error: "Unknown message type" });
    }
  },
);

async function handleAddWord({
  word,
  reading,
  meaning_en,
}: {
  word: string;
  reading: string;
  meaning_en: string;
}) {
  await ensureSrs();
  if (await getCard(word)) return { success: true, existing: true };
  const card = srs!.new_card(
    word,
    reading,
    meaning_en ?? "",
    Date.now(),
  ) as SrsCard;
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
  const updated = srs!.review_card(card, rating, Date.now()) as SrsCard;
  await putCard(updated);
  return { success: true, card: updated };
}

async function handleGetDue() {
  return { cards: await getDueCards(Date.now()) };
}

async function handleGetAllCards() {
  return { cards: await getAllCards() };
}

async function handleDeleteCard({ word }: { word: string }) {
  await deleteCard(word);
  return { success: true };
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
