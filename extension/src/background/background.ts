import type * as SrsWasm from "../../_generated/srs-wasm/srs_wasm.js";
import type * as KanjiWasm from "../../_generated/kanjidic-wasm/kanjidic_wasm.js";
import type * as ExamplesWasm from "../../_generated/examples-wasm/examples_wasm.js";
import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";
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
  deleteCardById,
  addLookupHistory,
  getAllTombstones,
  clearTombstones,
  replaceAllCards,
} from "./idb";
import { getSettings, saveSettings } from "./settings";
import { importCards, syncCardsBackup, writeCardsBackup } from "./cards-backup";
import type {
  CardDirection,
  ExampleEntry,
  KanjiEntry,
  SrsCard,
  SrsSettings,
  WordEntry,
} from "../shared/types.ts";
import { cardId } from "../shared/types.ts";
import {
  mergeReview,
  applyIntervalScale,
  checkGraduation,
  type SrsSchedFields,
} from "./review-utils.ts";

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
  const jsUrl = browser.runtime.getURL(
    "_generated/kanjidic-wasm/kanjidic_wasm.js",
  );
  const binUrl = browser.runtime.getURL(
    "_generated/kanjidic-wasm/kanjidic_wasm_bg.wasm",
  );
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
  const jsUrl = browser.runtime.getURL(
    "_generated/examples-wasm/examples_wasm.js",
  );
  const binUrl = browser.runtime.getURL(
    "_generated/examples-wasm/examples_wasm_bg.wasm",
  );
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
  const binUrl = browser.runtime.getURL(
    "_generated/jmdict-wasm/jmdict_wasm_bg.wasm",
  );
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
  scheduleSync();
}

// ── Auto-sync scheduler ───────────────────────────────────────────────
//
// Every card mutation flows through bumpDbVersion(), which calls
// scheduleSync(). We debounce 2 s and then POST cards+tombstones to the
// server. A separate IN_FLIGHT flag prevents overlapping requests; if a
// new mutation arrives during a sync, we kick off another pass when it
// finishes so no change is silently dropped.

const SYNC_DEBOUNCE_MS = 2_000;
let syncTimer: ReturnType<typeof setTimeout> | null = null;
let syncInFlight = false;
let syncRetry = false;

function scheduleSync(): void {
  if (syncInFlight) {
    syncRetry = true;
    return;
  }
  // Don't even arm the timer when the user has nothing configured —
  // saves a 2 s wait that ends in a no-op error log on every mutation.
  // We snapshot the token check via fire-and-forget; if the user
  // configures a server after this returns, the next mutation will
  // schedule properly.
  void getSettings().then((s) => {
    if (!s.serverUrl || !s.serverToken) return;
    if (syncInFlight) {
      syncRetry = true;
      return;
    }
    if (syncTimer) clearTimeout(syncTimer);
    syncTimer = setTimeout(() => {
      syncTimer = null;
      runSync().catch((e) => console.error("[yomeru] auto-sync failed:", e));
    }, SYNC_DEBOUNCE_MS);
  });
}

async function runSync(): Promise<void> {
  if (syncInFlight) return;
  syncInFlight = true;
  try {
    await doSync();
  } finally {
    syncInFlight = false;
    if (syncRetry) {
      syncRetry = false;
      scheduleSync();
    }
  }
}

async function doSync(): Promise<{ synced: number } | { error: string }> {
  const settings = await getSettings();
  if (!settings.serverUrl || !settings.serverToken) {
    return { error: "not authenticated" };
  }
  try {
    const allLocal = await getAllCards();
    const localTombstones = await getAllTombstones();
    // Only sequence-keyed cards can be represented server-side. Legacy
    // word-keyed rows (no numeric `sequence`) are left out of the upload so
    // they can't 422 the request; since the server is the source of truth and
    // its set replaces ours below, these unsyncable rows are dropped in the
    // process rather than lingering to poison the next sync.
    const upload = allLocal.filter(
      (c) => typeof c.sequence === "number" && Number.isFinite(c.sequence),
    );
    const res = await fetch(`${settings.serverUrl}/api/sync`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${settings.serverToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ cards: upload, deletions: localTombstones }),
    });
    if (res.status === 401) return { error: "session expired — re-verify" };
    if (!res.ok) return { error: `server ${res.status}` };
    const resp = (await res.json()) as { cards: SrsCard[] };
    // Server wins: adopt its merged set verbatim, discarding any local row it
    // didn't return (legacy junk, plus cards its last-write-wins merge rejected
    // as older). The cards we just uploaded come back in resp.cards, so valid
    // local-only cards aren't lost.
    await replaceAllCards(resp.cards);
    await clearTombstones(localTombstones);
    await writeCardsBackup();
    return { synced: resp.cards.length };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

// The message listener below waits on this before dispatching. A slow or
// *blocked* IndexedDB open (e.g. a stalled version upgrade) would otherwise
// leave it pending forever and wedge the entire message pipe — including auth
// and dict lookups, which don't even need the card store. Cap the wait so every
// message is still dispatched; card handlers re-open the DB themselves and
// surface their own errors if it's genuinely broken.
const storageReady = Promise.race([
  syncCardsBackup().catch((e) => {
    console.error("[yomeru] syncCardsBackup failed:", e);
  }),
  new Promise<void>((resolve) => setTimeout(resolve, 2000)),
]);

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

async function handleImportCards({ cards }: { cards: unknown }) {
  const result = await importCards(cards);
  if (result.added > 0) await bumpDbVersion();
  return result;
}

browser.runtime.onMessage.addListener(
  (msg: { type: string; payload?: unknown }) =>
    storageReady.then(() => dispatch(msg)),
);

// The Rust SrsCard the WASM works with is the FSRS scheduling subset plus
// `sequence` + `added_ms` — no id/direction/status. We never trust the
// JS-typed shape after a WASM round-trip; mergeReview reattaches the JS-only
// fields from the original card.
type WasmCardShape = SrsSchedFields & Pick<SrsCard, "sequence" | "added_ms">;

async function handleAddWord({ sequence }: { sequence: number }) {
  await ensureSrs();
  const siblings = await getCardsBySequence(sequence);
  if (siblings.length > 0) {
    return { success: true, existing: true };
  }
  const now = Date.now();
  const base = srs!.new_card(sequence, now) as WasmCardShape;
  const recognition: SrsCard = {
    ...base,
    id: cardId(sequence, "recognition"),
    sequence,
    direction: "recognition",
    status: "staging",
  };
  const recall: SrsCard = {
    ...base,
    id: cardId(sequence, "recall"),
    sequence,
    direction: "recall",
    status: "staging",
  };
  await putCards([recognition, recall]);
  await bumpDbVersion();
  return { success: true, existing: false };
}

async function handleReviewCard({
  sequence,
  direction,
  rating,
}: {
  sequence: number;
  direction: CardDirection;
  rating: number;
}) {
  await ensureSrs();
  const card = await getCard(sequence, direction);
  if (!card) return { error: "Card not found" };
  const settings = await getSettings();
  const now_ms = Date.now();
  const wasmOut = srs!.review_card(card, rating, now_ms) as WasmCardShape;
  const scaled = applyIntervalScale(wasmOut, settings.intervalScale, now_ms);
  if (checkGraduation(scaled.reps, settings.graduationReps)) {
    await deleteCardById(cardId(sequence, direction));
    await bumpDbVersion();
    return { success: true, graduated: true };
  }
  await putCard(mergeReview(card, scaled));
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

async function handlePromoteCard({ sequence }: { sequence: number }) {
  await promoteCard(sequence);
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

async function handleDeleteCard({ sequence }: { sequence: number }) {
  await deleteCard(sequence);
  await bumpDbVersion();
  return { success: true };
}

// Highlighting matches surface strings against page text, but cards now key on
// `sequence`. Resolve each active card's sequence to all of its kanji + reading
// surface forms so the content-script highlighter can keep underlining them.
async function handleGetSrsWords(): Promise<{ words: string[] }> {
  await ensureJmdict();
  const cards = await getAllCards();
  const seqs = [...new Set(cards.map((c) => c.sequence))];
  const entries = jmdict!.lookup_by_sequence(seqs) as (WordEntry | null)[];
  const words = new Set<string>();
  for (const e of entries) {
    if (!e) continue;
    for (const k of e.kanji_forms) words.add(k.text);
    for (const r of e.reading_forms) words.add(r.text);
  }
  return { words: [...words] };
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
  const entries = kanji!.lookup_many(word) as KanjiEntry[];
  return { entries: entries ?? [] };
}

async function handleGetExamples({ word }: { word: string }) {
  await ensureExamples();
  if (!examplesDict) return { entries: [] };
  const entries = examplesDict.lookup(word, 5) as ExampleEntry[];
  return { entries: entries ?? [] };
}

async function handleLookupWord({ word }: { word: string }) {
  await ensureJmdict();
  const entries = jmdict!.lookup(word) as WordEntry[];
  return { entries: entries ?? [] };
}

async function handleLookupMany({ words }: { words: string[] }) {
  await ensureJmdict();
  // The Rust shared crate's lookup_many returns Vec<Vec<WordEntry>> —
  // one entry list per input word, aligned by index. Mirror that shape.
  const results: WordEntry[][] = words.map(
    (w) => (jmdict!.lookup(w) as WordEntry[]) ?? [],
  );
  return { results };
}

async function handleLookupBySequence({ sequences }: { sequences: number[] }) {
  await ensureJmdict();
  const results = (jmdict!.lookup_by_sequence(
    sequences,
  ) as (WordEntry | null)[]) ?? [];
  return { results };
}

async function handleLookupPrefix({ text, max }: { text: string; max: number }) {
  await ensureJmdict();
  const results = (jmdict!.lookup_prefix(text, max) as WordEntry[]) ?? [];
  return { results };
}

async function handleRequestOtp({
  serverUrl,
  email,
}: {
  serverUrl: string;
  email: string;
}) {
  try {
    const res = await fetch(`${serverUrl}/api/auth/request`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email }),
    });
    if (!res.ok) return { error: `server ${res.status}` };
    // Dev mode: server skips OTP and returns a token directly (200 + JSON body).
    // Normal mode: returns 204 No Content.
    if (res.status === 200) {
      const { token } = (await res.json()) as { token: string };
      const settings = await getSettings();
      await saveSettings({ ...settings, serverEmail: email, serverToken: token });
      return { success: true, token };
    }
    return { success: true };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

async function handleVerifyOtp({
  serverUrl,
  email,
  code,
}: {
  serverUrl: string;
  email: string;
  code: string;
}) {
  try {
    const res = await fetch(`${serverUrl}/api/auth/verify`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email, code }),
    });
    if (!res.ok) return { error: `server ${res.status}: ${await res.text()}` };
    const { token } = (await res.json()) as { token: string };
    const settings = await getSettings();
    await saveSettings({ ...settings, serverToken: token });
    // Return the token so popup callers can update their local state without
    // waiting for the storage.onChanged event to propagate.
    return { success: true, token };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

async function handleSyncCards(): Promise<
  { synced: number } | { queued: true } | { error: string }
> {
  // Manual "Sync now" button: cancel any pending debounce and run
  // immediately. Mutations during the request are handled by the same
  // syncInFlight/syncRetry loop as scheduleSync.
  if (syncTimer) {
    clearTimeout(syncTimer);
    syncTimer = null;
  }
  if (syncInFlight) {
    // Don't start a concurrent request. Signal retry so the in-flight one
    // re-runs after itself, and tell the UI we queued the request (not an
    // error — the user's intent will be honored shortly).
    syncRetry = true;
    return { queued: true };
  }
  syncInFlight = true;
  try {
    return await doSync();
  } finally {
    syncInFlight = false;
    if (syncRetry) {
      syncRetry = false;
      scheduleSync();
    }
  }
}
