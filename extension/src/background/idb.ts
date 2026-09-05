import type { CardDirection, SrsCard } from "../shared/types.ts";
import { cardId, versionMs } from "../shared/types.ts";

const DB_NAME = "yomeru-db";
// v7 re-runs the v6 sequence-keyed reset to self-heal any DB left half-migrated
// by an earlier build that created the store without the `sequence` index.
const DB_VERSION = 7;
const TOMB_STORE = "tombstones";

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
      console.log(`[yomeru] idb upgrade ${e.oldVersion} → ${e.newVersion}`);

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
    };

    req.onsuccess = (e) => {
      const opened = (e.target as IDBOpenDBRequest).result;
      console.log(`[yomeru] idb opened at version ${opened.version}`);
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

export function putCard(card: SrsCard): Promise<IDBValidKey> {
  const c = stamped(card, Date.now());
  return tx("cards", "readwrite", (s) => s.put(c));
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

async function putAll(cards: SrsCard[], stamp: boolean): Promise<void> {
  if (cards.length === 0) return;
  const now = Date.now();
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readwrite");
    const store = t.objectStore("cards");
    for (const c of cards) store.put(stamp ? stamped(c, now) : c);
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
          resolve(all.filter((c) => c.status === "active"));
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

export async function promoteAll(): Promise<void> {
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readwrite");
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    const store = t.objectStore("cards");
    const req = store.index("status").openCursor(IDBKeyRange.only("staging"));
    req.onsuccess = (e) => {
      const cursor = (e.target as IDBRequest<IDBCursorWithValue>).result;
      if (!cursor) return;
      const card = cursor.value as SrsCard;
      cursor.update({ ...card, status: "active" });
      cursor.continue();
    };
    req.onerror = () => reject(req.error);
  });
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
): Promise<void> {
  if (resp.cards.length > 0) {
    await mergeRemoteCards(resp.cards);
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
 */
async function mergeRemoteCards(remote: SrsCard[]): Promise<void> {
  if (remote.length === 0) return;
  const local = await getAllCards();
  const localVersion = new Map<string, number>();
  for (const c of local) localVersion.set(c.id, versionMs(c));
  const toPut = remote.filter(
    (c) => versionMs(c) >= (localVersion.get(c.id) ?? 0),
  );
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

/**
 * Server-authoritative reset of the cards store: clears every local card and
 * writes the server's set verbatim, in a single transaction. Used by sync when
 * the server is the source of truth — replacing rather than merging also drops
 * any legacy word-keyed rows (no `sequence`) that can't be represented
 * server-side and would otherwise re-poison every upload.
 */
export async function replaceAllCards(cards: SrsCard[]): Promise<void> {
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readwrite");
    const store = t.objectStore("cards");
    store.clear();
    for (const c of cards) store.put(c);
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
