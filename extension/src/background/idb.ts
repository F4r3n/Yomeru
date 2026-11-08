import type { CardDirection, ReviewLog, SrsCard, WordStatus } from "../shared/types.ts";
import { cardId, MAX_PRIORITY, versionMs } from "../shared/types.ts";
import { debug } from "../shared/debug.ts";

const DB_NAME = "yomeru-db";
// v7 re-runs the v6 sequence-keyed reset to self-heal any DB left half-migrated
// by an earlier build that created the store without the `sequence` index.
// v8 adds the review outbox and the cached drill counts. v9 adds the delta-sync
// dirty set and the meta store holding the sync cursor. The app opens the same
// database, so this and `DB_VERSION` in app/shared/src/idb.rs move together.
const DB_VERSION = 9;
const TOMB_STORE = "tombstones";
const REVIEW_STORE = "review_outbox";
/** The server's per-card drill counts as of the last sync, `{card_id, count}`. */
const DRILL_STORE = "drill_counts";
/**
 * Cards changed locally since the last successful sync, `{id, v}` where `v` is
 * the `updated_ms` the change was stamped with. Written in the same
 * transaction as the card, so a change can't land without being queued.
 */
const DIRTY_STORE = "dirty";
/** Out-of-line keyed values that must live and die with the card data. */
const META_STORE = "meta";
const CURSOR_KEY = "sync_cursor";

let db: IDBDatabase | null = null;

/** A blank recall sibling, due immediately, in the "new" state. */
export function freshRecallCard(sequence: number, nowMs: number, addedMs: number): SrsCard {
  return {
    id: cardId(sequence, "recall"),
    sequence,
    direction: "recall",
    due_ms: nowMs,
    stability: 0,
    difficulty: 0,
    reps: 0,
    lapses: 0,
    state: "new",
    last_review_ms: null,
    added_ms: addedMs,
    status: "active",
    priority: 0,
  };
}

export async function openDb(): Promise<IDBDatabase> {
  if (db) return db;
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);

    req.onupgradeneeded = (e) => {
      const database = (e.target as IDBOpenDBRequest).result;
      const upgradeTx = (e.target as IDBOpenDBRequest).transaction!;
      debug(`idb upgrade ${e.oldVersion} → ${e.newVersion}`);

      upgradeTx.onerror = () => {
        console.error("[yomeru] idb upgrade tx error:", upgradeTx.error);
      };
      upgradeTx.onabort = () => {
        console.error("[yomeru] idb upgrade tx aborted:", upgradeTx.error);
      };

      // Fresh DB: create the cards store keyed on the composite id, indexed
      // by `sequence` (JMdict ent_seq) for sibling/by-entry lookups.
      const createCardsStore = () => {
        const cards = database.createObjectStore("cards", { keyPath: "id" });
        cards.createIndex("due_ms", "due_ms", { unique: false });
        cards.createIndex("added_ms", "added_ms", { unique: false });
        cards.createIndex("status", "status", { unique: false });
        cards.createIndex("sequence", "sequence", { unique: false });
      };

      if (e.oldVersion < 1) {
        createCardsStore();
        const history = database.createObjectStore("lookup_history", {
          keyPath: "id",
          autoIncrement: true,
        });
        history.createIndex("word", "word", { unique: false });
        history.createIndex("ts", "ts", { unique: false });
      }

      // → v6/v7: cards used to key on a surface `word` string; they now key on
      // JMdict `sequence`. There is deliberately no migration — the move is a
      // clean break and users re-import via export/import. Drop the old cards
      // store and recreate it empty on the `sequence` layout, and clear any
      // word-keyed tombstones (their composite ids are meaningless now). The
      // range also catches a half-migrated v6 (store created without the
      // `sequence` index by an earlier build) and rebuilds it cleanly.
      if (e.oldVersion >= 1 && e.oldVersion < 7) {
        if (database.objectStoreNames.contains("cards")) {
          database.deleteObjectStore("cards");
        }
        createCardsStore();
        if (database.objectStoreNames.contains(TOMB_STORE)) {
          database.deleteObjectStore(TOMB_STORE);
        }
      }

      // Ensure the tombstone store exists (new DBs and the v6 reset above).
      if (!database.objectStoreNames.contains(TOMB_STORE)) {
        database.createObjectStore(TOMB_STORE, { keyPath: "id" });
      }

      // → v8: answer log outbox and cached drill counts. Additive, so no data
      // is touched. Must match the Rust upgrade in app/shared/src/idb.rs.
      if (!database.objectStoreNames.contains(REVIEW_STORE)) {
        database.createObjectStore(REVIEW_STORE, { keyPath: ["card_id", "reviewed_ms"] });
      }
      if (!database.objectStoreNames.contains(DRILL_STORE)) {
        database.createObjectStore(DRILL_STORE, { keyPath: "card_id" });
      }

      // → v9: delta sync. Additive; with no cursor stored yet, the first sync
      // after the upgrade is a full one, so the dirty set can start empty.
      if (!database.objectStoreNames.contains(DIRTY_STORE)) {
        database.createObjectStore(DIRTY_STORE, { keyPath: "id" });
      }
      if (!database.objectStoreNames.contains(META_STORE)) {
        database.createObjectStore(META_STORE);
      }
    };

    req.onsuccess = (e) => {
      const opened = (e.target as IDBOpenDBRequest).result;
      debug(`idb opened at version ${opened.version}`);
      opened.onversionchange = () => {
        opened.close();
        if (db === opened) db = null;
      };
      opened.onclose = () => {
        if (db === opened) db = null;
      };
      db = opened;
      resolve(opened);
    };
    req.onerror = () => {
      console.error("[yomeru] idb open failed:", req.error);
      reject(req.error);
    };
    req.onblocked = () => {
      console.warn("[yomeru] idb open blocked — another connection holds an older version open");
    };
  });
}

async function tx<T>(
  store: string,
  mode: IDBTransactionMode,
  fn: (s: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  return openDb().then(
    (database) =>
      new Promise((resolve, reject) => {
        const t = database.transaction(store, mode);
        const req = fn(t.objectStore(store));
        let result: T;
        req.onsuccess = () => {
          result = req.result;
        };
        req.onerror = () => reject(req.error);
        t.oncomplete = () => resolve(result);
        t.onerror = () => reject(t.error);
        t.onabort = () => reject(t.error);
      }),
  );
}

/**
 * Stamps a card as written now. Every local mutation must advance the merge
 * key, and doing it at the single write point means reviews, promotions and
 * adds all get it without each call site remembering to.
 *
 * `mergeReview` and the various `{...c, status}` spreads carry the *old*
 * `updated_ms` forward, so without this stamp an extension review would upload
 * under a stale version and be silently discarded by the server.
 */
function stamped(card: SrsCard, now: number): SrsCard {
  return { ...card, updated_ms: now };
}

export async function putCard(card: SrsCard): Promise<void> {
  return putAll([card], true);
}

/**
 * Writes multiple cards in a single transaction. Atomic: if any put fails the
 * whole batch is rolled back, so we never end up with one sibling without the
 * other.
 */
export async function putCards(cards: SrsCard[]): Promise<void> {
  return putAll(cards, true);
}

/**
 * Writes cards adopted from the server verbatim, keeping the `updated_ms` they
 * arrived with. Only the sync merge should use this: re-stamping them would
 * mark every synced card as locally modified and bounce it back at the server
 * on the next round, forever.
 */
export async function putCardsSynced(cards: SrsCard[]): Promise<void> {
  return putAll(cards, false);
}

/**
 * A local write (`stamp`) is re-stamped and queued in the dirty set for the
 * next sync, in the same transaction. A write adopted from the server is
 * neither: it is already the server's state.
 */
async function putAll(cards: SrsCard[], stamp: boolean): Promise<void> {
  if (cards.length === 0) return;
  const now = Date.now();
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction(stamp ? ["cards", DIRTY_STORE] : ["cards"], "readwrite");
    const store = t.objectStore("cards");
    if (stamp) {
      const dirty = t.objectStore(DIRTY_STORE);
      for (const c of cards) {
        store.put(stamped(c, now));
        dirty.put({ id: c.id, v: now });
      }
    } else {
      for (const c of cards) store.put(c);
    }
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

export async function getCard(
  sequence: number,
  direction: CardDirection,
): Promise<SrsCard | null> {
  return tx<SrsCard | undefined>("cards", "readonly", (s) =>
    s.get(cardId(sequence, direction)),
  ).then((r) => r ?? null);
}

export async function getCardsBySequence(sequence: number): Promise<SrsCard[]> {
  return openDb().then(
    (database) =>
      new Promise((resolve, reject) => {
        const req = database
          .transaction("cards", "readonly")
          .objectStore("cards")
          .index("sequence")
          .getAll(IDBKeyRange.only(sequence));
        req.onsuccess = () => resolve(req.result as SrsCard[]);
        req.onerror = () => reject(req.error);
      }),
  );
}

export async function getAllCards(): Promise<SrsCard[]> {
  const cards = await tx<SrsCard[]>("cards", "readonly", (s) => s.getAll());
  // Cards stored before `updated_ms` existed come back without it. Resolve the
  // fallback here so callers — the sync upload above all — always hand out a
  // real merge version rather than one that loses every comparison.
  return cards.map((c) => ({ ...c, updated_ms: versionMs(c) }));
}

export async function getDueCards(nowMs: number): Promise<SrsCard[]> {
  return openDb().then(
    (database) =>
      new Promise((resolve, reject) => {
        const req = database
          .transaction("cards", "readonly")
          .objectStore("cards")
          .index("due_ms")
          .getAll(IDBKeyRange.upperBound(nowMs));
        req.onsuccess = () => {
          const all = req.result as SrsCard[];
          // Word cards only, like the app's `get_due_cards`: kanji cards are
          // reviewed in their own session.
          const due = all.filter((c) => c.status === "active" && c.direction !== "kanji");
          // Studied cards first, oldest-due first, then never-reviewed ones,
          // like the app's `order_due`: a new card's due_ms is only when it
          // was added, so it would otherwise crowd out real reviews.
          due.sort(
            (a, b) =>
              Number(a.state === "new") - Number(b.state === "new") || a.due_ms - b.due_ms,
          );
          resolve(due);
        };
        req.onerror = () => reject(req.error);
      }),
  );
}

export async function getStagingCards(): Promise<SrsCard[]> {
  return openDb().then(
    (database) =>
      new Promise((resolve, reject) => {
        const req = database
          .transaction("cards", "readonly")
          .objectStore("cards")
          .index("status")
          .getAll(IDBKeyRange.only("staging"));
        req.onsuccess = () => resolve(req.result as SrsCard[]);
        req.onerror = () => reject(req.error);
      }),
  );
}

/** Promotes both direction siblings of an entry from staging to active. */
export async function promoteCard(sequence: number): Promise<void> {
  const siblings = await getCardsBySequence(sequence);
  for (const c of siblings) {
    if (c.status === "staging") {
      await putCard({ ...c, status: "active" });
    }
  }
}

/**
 * Raises the priority of an entry's staged cards by one, up to
 * `MAX_PRIORITY`, pushing it up the New Words queue. Mirrors `bump_priority`
 * in `app/shared/src/idb.rs`.
 */
export async function bumpPriority(sequence: number): Promise<void> {
  const siblings = await getCardsBySequence(sequence);
  await putCards(
    siblings
      .filter((c) => c.status === "staging")
      .map((c) => ({ ...c, priority: Math.min(c.priority + 1, MAX_PRIORITY) })),
  );
}

/**
 * Sends an entry's active or graduated cards back to a fresh "new" state in
 * the active deck — how a graduated card re-enters Review. Destructive.
 * Mirrors `reset_card` / `SrsCard::reset_progression` in the app.
 */
export async function resetCard(sequence: number, nowMs: number): Promise<void> {
  const siblings = await getCardsBySequence(sequence);
  await putCards(
    siblings
      .filter((c) => c.status === "active" || c.status === "graduated")
      .map((c) => ({
        ...c,
        status: "active",
        due_ms: nowMs,
        stability: 0,
        difficulty: 0,
        reps: 0,
        lapses: 0,
        again_count: 0,
        state: "new",
        last_review_ms: null,
      })),
  );
}

/** An entry's SRS standing, summarised over its direction cards. */
export async function getWordStatus(sequence: number): Promise<WordStatus> {
  const siblings = await getCardsBySequence(sequence);
  const has = (s: SrsCard["status"]) => siblings.some((c) => c.status === s);
  const status = has("active")
    ? "active"
    : has("graduated")
      ? "graduated"
      : has("staging")
        ? "staging"
        : "none";
  const priority = Math.max(0, ...siblings.map((c) => c.priority ?? 0));
  return { status, priority };
}

/** Deletes both direction siblings for an entry and records tombstones. */
export async function deleteCard(sequence: number): Promise<void> {
  return deleteIdsWithTombstones([
    cardId(sequence, "recognition"),
    cardId(sequence, "recall"),
  ]);
}

/** Deletes a single sibling by composite id. */
export function deleteCardById(id: string): Promise<void> {
  return deleteIdsWithTombstones([id]);
}

/**
 * Atomically removes the given ids from the cards store and writes
 * tombstones for each. Keeping both writes in one transaction means a
 * crash mid-delete can't end up with the card gone but the tombstone
 * missing (which would silently undo the delete on the next sync).
 */
async function deleteIdsWithTombstones(ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction(["cards", TOMB_STORE], "readwrite");
    const cards = t.objectStore("cards");
    const tombs = t.objectStore(TOMB_STORE);
    const now = Date.now();
    for (const id of ids) {
      tombs.put({ id, deleted_at: now });
      cards.delete(id);
    }
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/**
 * A local record that the user deleted a card, and when. The time goes to the
 * server on the next sync so a delete made while offline is ordered by when it
 * actually happened rather than when it was finally uploaded.
 */
export interface Tombstone {
  id: string;
  deleted_at: number;
}

export async function getAllTombstones(): Promise<Tombstone[]> {
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const req = database
      .transaction(TOMB_STORE, "readonly")
      .objectStore(TOMB_STORE)
      .getAll();
    req.onsuccess = () =>
      resolve(
        (req.result as Tombstone[]).filter((t) => Boolean(t && t.id)),
      );
    req.onerror = () => reject(req.error);
  });
}

export async function clearTombstones(ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction(TOMB_STORE, "readwrite");
    const store = t.objectStore(TOMB_STORE);
    for (const id of ids) store.delete(id);
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/**
 * Saves a reviewed card and queues its answer for the next sync in one
 * transaction, so the log can neither record a review that didn't apply nor
 * miss one that did — even if the service worker is suspended mid-way. Stamps
 * the card like `putCard`. Mirrors `put_card_logged` in app/shared/src/idb.rs.
 */
export async function putCardLogged(card: SrsCard, row: ReviewLog): Promise<void> {
  const now = Date.now();
  const c = stamped(card, now);
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction(["cards", DIRTY_STORE, REVIEW_STORE], "readwrite");
    t.objectStore("cards").put(c);
    t.objectStore(DIRTY_STORE).put({ id: c.id, v: now });
    t.objectStore(REVIEW_STORE).put(row);
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/** Queues one answer for the next sync. */
export function addReview(row: ReviewLog): Promise<IDBValidKey> {
  return tx(REVIEW_STORE, "readwrite", (s) => s.put(row));
}

/** Every answer not yet acknowledged by the server. */
export function getOutbox(): Promise<ReviewLog[]> {
  return tx(REVIEW_STORE, "readonly", (s) => s.getAll() as IDBRequest<ReviewLog[]>);
}

/**
 * Settles an upload the server acknowledged: stores the server's drill counts
 * (which already include the sent rows) and drops exactly the rows that were
 * sent. Rows logged while the request was in flight have other keys and stay
 * queued. One transaction, so the counts and the outbox never disagree about
 * whether a row was counted.
 *
 * `replace`: the counts are the user's complete set (a full sync), so the
 * cache is cleared first. Otherwise they cover only the cards whose count
 * changed (a delta) and are merged over the cache.
 */
export async function settleOutbox(
  sent: ReviewLog[],
  drillCounts: Record<string, number>,
  replace: boolean,
): Promise<void> {
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction([REVIEW_STORE, DRILL_STORE], "readwrite");
    const outbox = t.objectStore(REVIEW_STORE);
    for (const r of sent) outbox.delete([r.card_id, r.reviewed_ms]);
    const counts = t.objectStore(DRILL_STORE);
    if (replace) counts.clear();
    for (const [card_id, count] of Object.entries(drillCounts)) {
      counts.put({ card_id, count });
    }
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/**
 * Applies a sync response into IDB:
 *   1. upserts the server's cards, skipping any whose local copy was written
 *      more recently (defends against clobbering a review, promotion or reset
 *      the user did while the sync was in flight),
 *   2. drops cards the server reports as deleted, except ones it hasn't ruled
 *      on yet — those we just sent tombstones for, and those added locally
 *      after we uploaded. Deleting either would silently eat a re-add,
 *   3. clears the tombstones we just forwarded (the server has them now).
 *
 * A tombstone the server still lists is a delete that stands: had a re-add
 * superseded it, the server's `upsert_cards` would have cleared it.
 *
 * `uploadedAt` must be the time captured *before* the request payload was read.
 * Pure data-shaping; no network. Exported so it's unit-testable against
 * fake-indexeddb.
 */
export async function applySyncResponse(
  resp: { cards: SrsCard[]; deletions?: string[] },
  sentTombstones: string[],
  uploadedAt: number,
  uploaded: SrsCard[] = [],
): Promise<void> {
  if (resp.cards.length > 0) {
    await mergeRemoteCards(resp.cards, uploaded);
  }
  if (resp.deletions && resp.deletions.length > 0) {
    const sent = new Set(sentTombstones);
    const localAdded = new Map<string, number>();
    for (const c of await getAllCards()) localAdded.set(c.id, c.added_ms);
    const foreign = resp.deletions.filter((id) => {
      if (sent.has(id)) return false;
      const added = localAdded.get(id);
      return added === undefined || added <= uploadedAt;
    });
    if (foreign.length > 0) {
      await applyRemoteDeletions(foreign);
    }
  }
  if (sentTombstones.length > 0) {
    await clearTombstones(sentTombstones);
  }
}

/**
 * Upserts `remote` cards into IDB, but skips any incoming card whose local copy
 * has a newer merge version — the user changed it after the sync request went
 * out, so the server's copy is stale. Mirrors the server-side `updated_ms`
 * last-write-wins check, and writes through `putCardsSynced` so adopted cards
 * keep the version they arrived with.
 *
 * One exception: a card this request `uploaded` and nobody has touched since
 * takes the server's copy whatever its version. That copy is the server's
 * ruling on the upload, and can carry an older stamp than the local one — the
 * server clamps a fast device clock's stamps to its own time. Keeping the
 * local copy there would leave it clean yet "newer" than everything the
 * server sends, so it would refuse other devices' edits until edited again.
 */
async function mergeRemoteCards(remote: SrsCard[], uploaded: SrsCard[]): Promise<void> {
  if (remote.length === 0) return;
  const local = await getAllCards();
  const localVersion = new Map<string, number>();
  for (const c of local) localVersion.set(c.id, versionMs(c));
  const uploadedVersion = new Map<string, number>();
  for (const c of uploaded) uploadedVersion.set(c.id, versionMs(c));
  const toPut = remote.filter((c) => {
    const mine = localVersion.get(c.id);
    if (mine === undefined) return true;
    if (uploadedVersion.get(c.id) === mine) return true;
    return versionMs(c) >= mine;
  });
  if (toPut.length > 0) await putCardsSynced(toPut);
}

/**
 * Deletes the given ids from the cards store without writing local
 * tombstones. Use this for server-driven deletes (the server already has
 * the tombstone — re-writing it locally would prevent the tombstone set
 * from converging across syncs).
 */
export async function applyRemoteDeletions(ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readwrite");
    const store = t.objectStore("cards");
    for (const id of ids) store.delete(id);
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

export function addLookupHistory(
  word: string,
  reading: string,
): Promise<IDBValidKey> {
  return tx("lookup_history", "readwrite", (s) =>
    s.add({ word, reading, ts: Date.now() }),
  );
}

// ── Delta sync state ──────────────────────────────────────────────────

/** One dirty-set entry: card `id`, changed at stamp `v`. */
export interface DirtyEntry {
  id: string;
  v: number;
}

/** The dirty set as it stands, to upload and later hand to {@link clearDirty}. */
export function getDirty(): Promise<DirtyEntry[]> {
  return tx(DIRTY_STORE, "readonly", (s) => s.getAll() as IDBRequest<DirtyEntry[]>);
}

/** The cards behind a dirty snapshot. Ids deleted since are skipped. */
export async function getCardsByIds(ids: string[]): Promise<SrsCard[]> {
  if (ids.length === 0) return [];
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readonly");
    const store = t.objectStore("cards");
    const out: SrsCard[] = [];
    for (const id of ids) {
      const req = store.get(id);
      req.onsuccess = () => {
        if (req.result) out.push(req.result as SrsCard);
      };
    }
    t.oncomplete = () => resolve(out);
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/**
 * Drops the entries of an uploaded snapshot, except any re-stamped since it
 * was taken: that card changed while the sync was in flight, and its new
 * state still has to go up.
 */
export async function clearDirty(snapshot: DirtyEntry[]): Promise<void> {
  if (snapshot.length === 0) return;
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction(DIRTY_STORE, "readwrite");
    const store = t.objectStore(DIRTY_STORE);
    for (const e of snapshot) {
      const req = store.get(e.id);
      req.onsuccess = () => {
        const cur = req.result as DirtyEntry | undefined;
        if (cur && cur.v === e.v) store.delete(e.id);
      };
    }
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/**
 * Where this device's view of the server ends. Opaque: sent back as `since`.
 * Kept in IndexedDB beside the cards on purpose — if the database is wiped,
 * the cursor goes with it and the next sync is a full one. A cursor that
 * outlived its data would ask for a delta and never get the lost cards back.
 */
export interface SyncCursor {
  epoch: string;
  rev: number;
}

export async function getSyncCursor(): Promise<SyncCursor | null> {
  const v = await tx(META_STORE, "readonly", (s) => s.get(CURSOR_KEY) as IDBRequest<unknown>);
  const c = v as Partial<SyncCursor> | undefined;
  return c && typeof c.epoch === "string" && typeof c.rev === "number"
    ? { epoch: c.epoch, rev: c.rev }
    : null;
}

export async function setSyncCursor(c: SyncCursor): Promise<void> {
  await tx(META_STORE, "readwrite", (s) => s.put(c, CURSOR_KEY));
}

/** Forgets the cursor, so the next sync is a full one. */
export async function clearSyncCursor(): Promise<void> {
  await tx(META_STORE, "readwrite", (s) => s.delete(CURSOR_KEY));
}
