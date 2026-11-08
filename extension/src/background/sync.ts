/**
 * Auto-sync: the debounced push of cards, tombstones and the review outbox to
 * the server, plus the account handshake that authorises it.
 *
 * Syncs are deltas once the device holds a cursor: only cards in the dirty set
 * go up, and only what changed after the cursor (plus the server's ruling on
 * each id sent) comes back. Without a cursor — first sync, new account, wiped
 * database, or an old server that sends none — the whole deck goes up and the
 * server answers with everything.
 *
 * Every card mutation calls `bumpDbVersion`, which schedules a sync. A single
 * in-flight flag keeps requests from overlapping, and a retry flag makes sure a
 * mutation that lands mid-request still gets pushed once that request finishes,
 * so no change is silently dropped.
 */

import {
  getAllCards,
  getAllTombstones,
  applyRemoteDeletions,
  applySyncResponse,
  clearDirty,
  clearSyncCursor,
  getCardsByIds,
  getDirty,
  getOutbox,
  getSyncCursor,
  setSyncCursor,
  settleOutbox,
  type SyncCursor,
} from "./idb";
import {
  adoptRemoteSettings,
  getSettings,
  saveSettings,
  toSettingsPayload,
  type SettingsPayload,
} from "./settings";
import { syncCardsBackup, writeCardsBackup } from "./cards-backup";
import { hasSyncConsent, NO_CONSENT_ERROR } from "./consent";
import type { SrsCard } from "../shared/types.ts";

export async function bumpDbVersion(): Promise<void> {
  await Promise.all([
    browser.storage.local.set({ _yomeru_db_v: Date.now() }),
    writeCardsBackup(),
  ]);
  scheduleSync();
}

// ── Auto-sync scheduler ───────────────────────────────────────────────
//
// Every card mutation flows through bumpDbVersion(), which calls
// scheduleSync(). We debounce 2 s and then POST cards+tombstones (and any
// locally edited scheduler settings) to the
// server. A separate IN_FLIGHT flag prevents overlapping requests; if a
// new mutation arrives during a sync, we kick off another pass when it
// finishes so no change is silently dropped.

const SYNC_DEBOUNCE_MS = 2_000;
/**
 * Most review-log rows one sync uploads, so a backlog never outgrows the
 * server's request body limit; the rest go up on the following syncs. Mirrors
 * `OUTBOX_BATCH` in app/shared/src/sync.rs.
 */
export const OUTBOX_BATCH = 1_000;
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
  void Promise.all([getSettings(), hasSyncConsent()]).then(([s, consent]) => {
    if (!s.serverUrl || !s.serverToken || !consent) return;
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

/**
 * storage.local key holding the outcome of the latest sync attempt. The
 * options page watches it to show its "not synced" banner, which is the only
 * way it learns that a background auto-sync failed.
 */
export const SYNC_STATUS_KEY = "_yomeru_sync_status";

/** Runs a sync and publishes its outcome under {@link SYNC_STATUS_KEY}. */
async function syncAndReport(): Promise<{ synced: number; sent: number } | { error: string }> {
  const result = await doSync();
  const error = "error" in result ? result.error : null;
  await browser.storage.local
    .set({ [SYNC_STATUS_KEY]: { error, at: Date.now() } })
    .catch((e) => console.error("[yomeru] write sync status failed:", e));
  return result;
}

async function runSync(): Promise<void> {
  if (syncInFlight) return;
  syncInFlight = true;
  try {
    const result = await syncAndReport();
    if ("error" in result) console.warn("[yomeru] auto-sync failed:", result.error);
  } finally {
    syncInFlight = false;
    if (syncRetry) {
      syncRetry = false;
      scheduleSync();
    }
  }
}

async function doSync(): Promise<{ synced: number; sent: number } | { error: string }> {
  const settings = await getSettings();
  if (!settings.serverUrl || !settings.serverToken) {
    return { error: "not authenticated" };
  }
  if (!(await hasSyncConsent())) return { error: NO_CONSENT_ERROR };
  try {
    // Captured before the reads, not after: a card written between the read and
    // the timestamp would be missing from the payload yet still look like the
    // server had ruled on it, and a returned tombstone would delete it.
    const uploadedAt = Date.now();
    const cursor = await getSyncCursor();
    // Read before the cards, so a card changed after this snapshot either
    // misses the upload *and* stays dirty, or is in both.
    const dirty = await getDirty();
    const allLocal = cursor
      ? await getCardsByIds(dirty.map((d) => d.id))
      : await getAllCards();
    const localTombstones = await getAllTombstones();
    // Oldest first: the store's key order is by card id, not by time.
    const pending = (await getOutbox()).sort((a, b) => a.reviewed_ms - b.reviewed_ms);
    const outbox = pending.slice(0, OUTBOX_BATCH);
    // Only sequence-keyed cards can be represented server-side. Legacy
    // word-keyed rows (no numeric `sequence`) are left out of the upload so
    // they can't 422 the request.
    const syncable = (c: SrsCard) =>
      typeof c.sequence === "number" && Number.isFinite(c.sequence);
    const upload = allLocal.filter(syncable);
    const res = await fetch(`${settings.serverUrl}/api/sync`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${settings.serverToken}`,
        "Content-Type": "application/json",
      },
      // Tombstones go as {id, deleted_at} objects so a delete made offline is
      // ordered by when it happened, not when it reached the server. The server
      // still accepts the bare-id form older clients send, so it must be
      // deployed before this client ships.
      body: JSON.stringify({
        cards: upload,
        deletions: localTombstones,
        settings: toSettingsPayload(settings),
        reviews: outbox,
        ...(cursor ? { since: cursor } : {}),
      }),
    });
    // Must match `SESSION_EXPIRED` in app/shared/src/sync.rs, which keys the
    // "sign in again" banner off this exact string.
    if (res.status === 401) return { error: "session expired — re-verify" };
    if (!res.ok) return { error: `server ${res.status}` };
    const resp = (await res.json()) as {
      cards: SrsCard[];
      deletions?: string[];
      settings?: SettingsPayload;
      drill_counts?: Record<string, number>;
      cursor?: SyncCursor;
      full?: boolean;
    };
    // A server that predates delta sync sends neither field, and its answer is
    // always the complete state.
    const full = resp.full ?? true;
    // Merge rather than replace. Wholesale replacement looks server-authoritative
    // but loses every write that landed during the round trip: a card reviewed
    // mid-flight comes back carrying the server's older copy and is overwritten,
    // and a card *added* mid-flight was never in the upload, so it isn't in the
    // response either, and replacement deletes it. applySyncResponse applies the
    // same last-write-wins and re-add guards the Rust client uses.
    await applySyncResponse(
      resp,
      localTombstones.map((t) => t.id),
      uploadedAt,
      upload,
    );
    // The one thing replacement did that merging doesn't: drop legacy
    // word-keyed rows the server can never return, since they were never
    // uploaded. They're local-only junk, so no tombstone — nothing else has a
    // copy to resurrect.
    const legacy = allLocal.filter((c) => !syncable(c)).map((c) => c.id);
    await applyRemoteDeletions(legacy);
    if (resp.settings) await adoptRemoteSettings(resp.settings);
    // A server without the review log sends no counts and kept none of the
    // rows, so the outbox waits for one that does.
    const settled = resp.drill_counts !== undefined;
    if (resp.drill_counts) await settleOutbox(outbox, resp.drill_counts, full);
    await clearDirty(dirty);
    // Last: if anything above failed, the old cursor stays and the next sync
    // asks for these rows again, which merging makes harmless.
    //
    // A full answer to a request that carried a cursor means the server
    // couldn't serve a delta from it: its database was restored or reset, or
    // the cursor belongs to another account. Either way it may lack cards that
    // only this device holds, and this request sent only the dirty ones. So
    // keep no cursor and sync again at once, uploading the whole deck.
    const refused = cursor !== null && full;
    if (refused || !resp.cursor) await clearSyncCursor();
    else await setSyncCursor(resp.cursor);
    if (refused) syncRetry = true;
    await writeCardsBackup();
    // Drain a backlog on the next pass (runSync re-arms on this flag) — only
    // from a sync that fully succeeded, so a failing one can't loop.
    if (settled && pending.length > outbox.length) syncRetry = true;
    // `synced` counts cards received, `sent` cards uploaded: what crossed the
    // wire, not the deck size.
    return { synced: resp.cards.length, sent: upload.length };
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
export const storageReady = Promise.race([
  syncCardsBackup().catch((e) => {
    console.error("[yomeru] syncCardsBackup failed:", e);
  }),
  new Promise<void>((resolve) => setTimeout(resolve, 2000)),
]);

export async function handleRequestOtp({
  serverUrl,
  email,
}: {
  serverUrl: string;
  email: string;
}) {
  if (!(await hasSyncConsent())) return { error: NO_CONSENT_ERROR };
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
      await clearSyncCursor();
      return { success: true, token };
    }
    return { success: true };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

export async function handleVerifyOtp({
  serverUrl,
  email,
  code,
}: {
  serverUrl: string;
  email: string;
  code: string;
}) {
  if (!(await hasSyncConsent())) return { error: NO_CONSENT_ERROR };
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
    // Possibly another account, or another server: the cursor described what
    // this device had seen of the old one.
    await clearSyncCursor();
    // Return the token so popup callers can update their local state without
    // waiting for the storage.onChanged event to propagate.
    return { success: true, token };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

export async function handleSyncCards(): Promise<
  { synced: number; sent: number } | { queued: true } | { error: string }
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
    return await syncAndReport();
  } finally {
    syncInFlight = false;
    if (syncRetry) {
      syncRetry = false;
      scheduleSync();
    }
  }
}
