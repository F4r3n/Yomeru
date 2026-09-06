/**
 * Service-worker entry point: warms the engines, keeps the toolbar icon in step
 * with the enabled flag, and routes incoming messages to a handler.
 *
 * The work itself lives in `engines.ts` (WASM lifecycle), `sync.ts` (server
 * reconciliation) and `handlers.ts` (one function per message type); this file
 * is the wiring.
 */

import { warmUp } from "./engines";
import { bumpDbVersion, storageReady, handleRequestOtp, handleSyncCards, handleVerifyOtp } from "./sync";
import {
  handleAddWord,
  handleDeleteCard,
  handleGetAllCards,
  handleGetDue,
  handleGetExamples,
  handleGetKanji,
  handleGetSettings,
  handleGetSrsWords,
  handleGetStaging,
  handleImportCards,
  handleLogLookup,
  handleLookupBySequence,
  handleLookupMany,
  handleLookupPrefix,
  handleLookupWord,
  handlePromoteAll,
  handlePromoteBatch,
  handlePromoteCard,
  handleReviewCard,
  handleSaveSettings,
} from "./handlers";
import type { CardDirection, SrsSettings } from "../shared/types.ts";

warmUp();

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
      return handleAddWord(msg.payload as { sequence: number });
    case "REVIEW_CARD":
      return handleReviewCard(
        msg.payload as {
          sequence: number;
          direction: CardDirection;
          rating: number;
        },
      );
    case "GET_DUE":
      return handleGetDue();
    case "GET_ALL_CARDS":
      return handleGetAllCards();
    case "DELETE_CARD":
      return handleDeleteCard(msg.payload as { sequence: number });
    case "LOG_LOOKUP":
      return handleLogLookup(msg.payload as { word: string; reading: string });
    case "GET_SRS_WORDS":
      return handleGetSrsWords();
    case "GET_STAGING":
      return handleGetStaging();
    case "PROMOTE_CARD":
      return handlePromoteCard(msg.payload as { sequence: number });
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
    case "LOOKUP_MANY":
      return handleLookupMany(msg.payload as { words: string[] });
    case "LOOKUP_BY_SEQUENCE":
      return handleLookupBySequence(msg.payload as { sequences: number[] });
    case "LOOKUP_PREFIX":
      return handleLookupPrefix(msg.payload as { text: string; max: number });
    case "BUMP_DB_VERSION":
      return bumpDbVersion().then(() => ({ ok: true }));
    case "IMPORT_CARDS":
      return handleImportCards(msg.payload as { cards: unknown });
    case "REQUEST_OTP":
      return handleRequestOtp(msg.payload as { serverUrl: string; email: string });
    case "VERIFY_OTP":
      return handleVerifyOtp(
        msg.payload as { serverUrl: string; email: string; code: string },
      );
    case "SYNC_CARDS":
      return handleSyncCards();
    default:
      return Promise.resolve({ error: "Unknown message type" });
  }
}

browser.runtime.onMessage.addListener(
  (msg: { type: string; payload?: unknown }) =>
    storageReady.then(() => dispatch(msg)),
);
