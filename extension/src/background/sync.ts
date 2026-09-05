/**
 * Auto-sync: the debounced push of cards and tombstones to the server, plus the
 * account handshake that authorises it.
 *
 * Every card mutation calls `bumpDbVersion`, which schedules a sync. A single
 * in-flight flag keeps requests from overlapping, and a retry flag makes sure a
 * mutation that lands mid-request still gets pushed once that request finishes,
 * so no change is silently dropped.
 */

import { getAllCards, getAllTombstones, clearTombstones, replaceAllCards } from "./idb";
import { getSettings, saveSettings } from "./settings";
import { syncCardsBackup, writeCardsBackup } from "./cards-backup";
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
      // Tombstones go as {id, deleted_at} objects so a delete made offline is
      // ordered by when it happened, not when it reached the server. The server
      // still accepts the bare-id form older clients send, so it must be
      // deployed before this client ships.
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
    await clearTombstones(localTombstones.map((t) => t.id));
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

export async function handleVerifyOtp({
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

export async function handleSyncCards(): Promise<
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
