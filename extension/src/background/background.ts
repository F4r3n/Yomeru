/**
 * Service-worker entry point: warms the engines, keeps the toolbar icon in step
 * with the enabled flag, and routes incoming messages to a handler.
 *
 * The work itself lives in `engines.ts` (WASM lifecycle), `sync.ts` (server
 * reconciliation) and `handlers.ts` (one function per message type); this file
 * is the wiring.
 */

import { hasSyncConsent } from "./consent";
import { applyDevConfig } from "./dev-config";
import { warmUp } from "./engines";
import { bumpDbVersion, storageReady, handleRequestOtp, handleSyncCards, handleVerifyOtp } from "./sync";
import {
  handleAddWord,
  handleAddGrammar,
  handleGrammarBySequence,
  handleGrammarCheck,
  handleSentenceWords,
  handleDeleteCard,
  handleGetAllCards,
  handleGetDue,
  handleGetExamples,
  handleGrammarLookup,
  handleGrammarSearch,
  handleGetKanji,
  handleGetKanjiParts,
  handleKanjiByParts,
  handleGetSettings,
  handleGetSrsSequences,
  handleGetStaging,
  handleImportCards,
  handleLogLookup,
  handleLookupBySequence,
  handleLookupMany,
  handleLookupPrefix,
  handleLookupWord,
  handlePromoteCard,
  handleBumpPriority,
  handleResetCard,
  handleGetWordStatus,
  handleReviewCard,
  handleSaveSettings,
} from "./handlers";
import type { CardDirection, CardKey, CardKind, SrsSettings } from "../shared/types.ts";

warmUp();
void applyDevConfig();

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
    case "ADD_GRAMMAR":
      return handleAddGrammar(msg.payload as { seq: number });
    case "REVIEW_CARD":
      return handleReviewCard(
        msg.payload as CardKey & {
          direction: CardDirection;
          rating: number;
        },
      );
    case "GET_DUE":
      return handleGetDue();
    case "GET_ALL_CARDS":
      return handleGetAllCards();
    case "DELETE_CARD":
      return handleDeleteCard(msg.payload as CardKey);
    case "LOG_LOOKUP":
      return handleLogLookup(msg.payload as { word: string; reading: string });
    case "GET_SRS_SEQUENCES":
      return handleGetSrsSequences();
    case "GET_STAGING":
      return handleGetStaging();
    case "PROMOTE_CARD":
      return handlePromoteCard(msg.payload as CardKey);
    case "BUMP_PRIORITY":
      return handleBumpPriority(msg.payload as CardKey);
    case "RESET_CARD":
      return handleResetCard(msg.payload as CardKey);
    case "GET_WORD_STATUS":
      return handleGetWordStatus(msg.payload as { kind: CardKind; items: string[] });
    case "GET_SETTINGS":
      return handleGetSettings();
    case "SAVE_SETTINGS":
      return handleSaveSettings(msg.payload as SrsSettings);
    case "GET_KANJI":
      return handleGetKanji(msg.payload as { word: string });
    case "GET_KANJI_PARTS":
      return handleGetKanjiParts();
    case "KANJI_BY_PARTS":
      return handleKanjiByParts(msg.payload as { parts: string; max: number });
    case "GET_EXAMPLES":
      return handleGetExamples(msg.payload as { word: string });
    case "GRAMMAR_LOOKUP":
      return handleGrammarLookup(msg.payload as { text: string });
    case "GRAMMAR_SEARCH":
      return handleGrammarSearch(msg.payload as { text: string; max: number });
    case "LOOKUP_WORD":
      return handleLookupWord(msg.payload as { word: string });
    case "LOOKUP_MANY":
      return handleLookupMany(msg.payload as { words: string[] });
    case "LOOKUP_BY_SEQUENCE":
      return handleLookupBySequence(msg.payload as { sequences: number[] });
    case "GRAMMAR_BY_SEQUENCE":
      return handleGrammarBySequence(msg.payload as { sequences: number[] });
    case "SENTENCE_WORDS":
      return handleSentenceWords(msg.payload as { text: string });
    case "GRAMMAR_CHECK":
      return handleGrammarCheck(msg.payload as { id: string; answer: string });
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
    case "GET_SYNC_CONSENT":
      return hasSyncConsent().then((granted) => ({ granted }));
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
