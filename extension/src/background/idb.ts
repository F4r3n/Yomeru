import type { CardDirection, CardState, SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

const DB_NAME = "yomeru-db";
const DB_VERSION = 5;
const TOMB_STORE = "tombstones";

let db: IDBDatabase | null = null;

/**
 * Maps a legacy SM-2-shaped card object to the FSRS scheduling fields.
 * Exported so cards-backup can apply the same conversion on legacy JSON imports
 * and on legacy storage.local backups.
 */
export function sm2ToFsrsFields(old: Record<string, unknown>): {
  stability: number;
  difficulty: number;
  reps: number;
  lapses: number;
  state: CardState;
  last_review_ms: number | null;
} {
  const num = (v: unknown, fallback: number) =>
    typeof v === "number" && Number.isFinite(v) ? v : fallback;
  const interval = num(old.interval_days, 0);
  const ease = num(old.ease_factor, 2.5) || 2.5;
  const reps = num(old.repetitions, 0);
  // SM-2 ease (1.3..2.5+) → FSRS difficulty (1..10, higher = harder).
  // ease=2.5 → ~1, ease=1.3 → 10, linear in between.
  const difficulty = Math.max(1, Math.min(10, 10 - (ease - 1.3) / 0.12));
  return {
    stability: reps > 0 ? Math.max(0.1, interval) : 0,
    difficulty: reps > 0 ? difficulty : 0,
    reps,
    lapses: 0,
    state: reps > 0 ? "review" : "new",
    last_review_ms: typeof old.last_reviewed_ms === "number" ? old.last_reviewed_ms : null,
  };
}

/** A blank recall sibling, due immediately, in the "new" state. */
export function freshRecallCard(word: string, nowMs: number, addedMs: number): SrsCard {
  return {
    id: cardId(word, "recall"),
    word,
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

      if (e.oldVersion < 1) {
        const cards = database.createObjectStore("cards", { keyPath: "id" });
        cards.createIndex("due_ms", "due_ms", { unique: false });
        cards.createIndex("added_ms", "added_ms", { unique: false });
        cards.createIndex("status", "status", { unique: false });
        cards.createIndex("word", "word", { unique: false });

        const history = database.createObjectStore("lookup_history", {
          keyPath: "id",
          autoIncrement: true,
        });
        history.createIndex("word", "word", { unique: false });
        history.createIndex("ts", "ts", { unique: false });
      }

      // v1/v2 → v4: rebuild cards store with composite-id keyPath, spawn
      // recall sibling for every existing card, and write FSRS-shaped fields.
      // This branch also subsumes the legacy "fill missing status" step.
      if (e.oldVersion >= 1 && e.oldVersion < 3) {
        const oldStore = upgradeTx.objectStore("cards");
        const collected: Array<Record<string, unknown>> = [];
        const cursorReq = oldStore.openCursor();
        cursorReq.onerror = () => {
          console.error("[yomeru] v1/v2→v4 cursor error:", cursorReq.error);
        };
        cursorReq.onsuccess = (ev) => {
          const cursor = (ev.target as IDBRequest<IDBCursorWithValue>).result;
          if (cursor) {
            collected.push(cursor.value);
            cursor.continue();
            return;
          }
          console.log(`[yomeru] v1/v2→v4 collected ${collected.length} legacy cards`);
          database.deleteObjectStore("cards");
          const newStore = database.createObjectStore("cards", { keyPath: "id" });
          newStore.createIndex("due_ms", "due_ms", { unique: false });
          newStore.createIndex("added_ms", "added_ms", { unique: false });
          newStore.createIndex("status", "status", { unique: false });
          newStore.createIndex("word", "word", { unique: false });

          const now = Date.now();
          const num = (v: unknown, fallback: number) =>
            typeof v === "number" && Number.isFinite(v) ? v : fallback;
          for (const old of collected) {
            const word = String(old.word);
            const fsrs = sm2ToFsrsFields(old);
            const recognition: SrsCard = {
              id: cardId(word, "recognition"),
              word,
              direction: "recognition",
              due_ms: num(old.due_ms, now),
              ...fsrs,
              added_ms: num(old.added_ms, now),
              status: (old.status as SrsCard["status"]) ?? "active",
            };
            newStore.add(recognition);
            newStore.add(freshRecallCard(word, now, recognition.added_ms));
          }
        };
      }

      // v4 → v5: add a tombstone store for sync. Existing cards store is
      // untouched; the new store keeps {id, deleted_at} entries so deletes
      // can propagate to the server and other devices.
      if (e.oldVersion < 5 && !database.objectStoreNames.contains(TOMB_STORE)) {
        database.createObjectStore(TOMB_STORE, { keyPath: "id" });
      }

      // v3 → v4: store already has composite-id keyPath and sibling pairs, but
      // cards carry SM-2 fields (interval_days, ease_factor, repetitions).
      // Walk every card and rewrite to FSRS-shaped fields in place.
      if (e.oldVersion >= 3 && e.oldVersion < 4) {
        const store = upgradeTx.objectStore("cards");
        let migrated = 0;
        const cursorReq = store.openCursor();
        cursorReq.onerror = () => {
          console.error("[yomeru] v3→v4 cursor error:", cursorReq.error);
        };
        cursorReq.onsuccess = (ev) => {
          const cursor = (ev.target as IDBRequest<IDBCursorWithValue>).result;
          if (!cursor) {
            console.log(`[yomeru] v3→v4 rewrote ${migrated} cards`);
            return;
          }
          const old = cursor.value as Record<string, unknown>;
          const fsrs = sm2ToFsrsFields(old);
          const updated: SrsCard = {
            id: old.id as string,
            word: old.word as string,
            direction: old.direction as CardDirection,
            due_ms: typeof old.due_ms === "number" ? old.due_ms : Date.now(),
            ...fsrs,
            added_ms: typeof old.added_ms === "number" ? old.added_ms : Date.now(),
            status: (old.status as SrsCard["status"]) ?? "active",
          };
          const upd = cursor.update(updated);
          upd.onerror = () => {
            console.error(
              `[yomeru] v3→v4 cursor.update failed for id=${updated.id}:`,
              upd.error,
              "value:",
              updated,
            );
          };
          migrated++;
          cursor.continue();
        };
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

export function putCard(card: SrsCard): Promise<IDBValidKey> {
  return tx("cards", "readwrite", (s) => s.put(card));
}

/**
 * Writes multiple cards in a single transaction. Atomic: if any put fails the
 * whole batch is rolled back, so we never end up with one sibling without the
 * other.
 */
export async function putCards(cards: SrsCard[]): Promise<void> {
  if (cards.length === 0) return;
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readwrite");
    const store = t.objectStore("cards");
    for (const c of cards) store.put(c);
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

export async function getCard(
  word: string,
  direction: CardDirection,
): Promise<SrsCard | null> {
  return tx<SrsCard | undefined>("cards", "readonly", (s) =>
    s.get(cardId(word, direction)),
  ).then((r) => r ?? null);
}

export async function getCardsByWord(word: string): Promise<SrsCard[]> {
  return openDb().then(
    (database) =>
      new Promise((resolve, reject) => {
        const req = database
          .transaction("cards", "readonly")
          .objectStore("cards")
          .index("word")
          .getAll(IDBKeyRange.only(word));
        req.onsuccess = () => resolve(req.result as SrsCard[]);
        req.onerror = () => reject(req.error);
      }),
  );
}

export function getAllCards(): Promise<SrsCard[]> {
  return tx<SrsCard[]>("cards", "readonly", (s) => s.getAll());
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

/** Promotes both direction siblings of a word from staging to active. */
export async function promoteCard(word: string): Promise<void> {
  const siblings = await getCardsByWord(word);
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

/** Deletes both direction siblings for a word and records tombstones. */
export async function deleteCard(word: string): Promise<void> {
  return deleteIdsWithTombstones([
    cardId(word, "recognition"),
    cardId(word, "recall"),
  ]);
}

/** Deletes a single sibling by composite id (used when one direction graduates). */
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

export async function getAllTombstones(): Promise<string[]> {
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const req = database
      .transaction(TOMB_STORE, "readonly")
      .objectStore(TOMB_STORE)
      .getAll();
    req.onsuccess = () =>
      resolve(
        (req.result as Array<{ id: string }>).map((r) => r.id).filter(Boolean),
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
 *   1. upserts the server's cards, skipping any whose local copy has a
 *      newer `last_review_ms` (defends against clobbering a review the
 *      user did while the sync was in flight),
 *   2. drops cards the server reports as deleted *unless* we just sent a
 *      tombstone for that id — if we did, the local cards store either
 *      already lacks the id or holds an intentional re-add by the user,
 *      and either way it would be wrong to delete it,
 *   3. clears the tombstones we just forwarded (the server has them now).
 *
 * Pure data-shaping; no network. Exported so it's unit-testable against
 * fake-indexeddb.
 */
export async function applySyncResponse(
  resp: { cards: SrsCard[]; deletions?: string[] },
  sentTombstones: string[],
): Promise<void> {
  if (resp.cards.length > 0) {
    await putCardsSkipOlder(resp.cards);
  }
  if (resp.deletions && resp.deletions.length > 0) {
    const sent = new Set(sentTombstones);
    const foreign = resp.deletions.filter((id) => !sent.has(id));
    if (foreign.length > 0) {
      await applyRemoteDeletions(foreign);
    }
  }
  if (sentTombstones.length > 0) {
    await clearTombstones(sentTombstones);
  }
}

/**
 * Upserts `remote` cards into IDB, but skips any incoming card whose
 * `last_review_ms` is older than what we already have locally — that means
 * the user reviewed the card after the sync request went out, and the
 * server's copy is stale. Mirrors the server-side last-write-wins check.
 */
async function putCardsSkipOlder(remote: SrsCard[]): Promise<void> {
  if (remote.length === 0) return;
  const local = await getAllCards();
  const localTs = new Map<string, number>();
  for (const c of local) localTs.set(c.id, c.last_review_ms ?? 0);
  const toPut = remote.filter(
    (c) => (c.last_review_ms ?? 0) >= (localTs.get(c.id) ?? 0),
  );
  if (toPut.length > 0) await putCards(toPut);
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
