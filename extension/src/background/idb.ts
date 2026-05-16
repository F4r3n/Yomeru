import type { CardDirection, SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

const DB_NAME = "yomeru-db";
const DB_VERSION = 3;

let db: IDBDatabase | null = null;

export async function openDb(): Promise<IDBDatabase> {
  if (db) return db;
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);

    req.onupgradeneeded = (e) => {
      const database = (e.target as IDBOpenDBRequest).result;
      const upgradeTx = (e.target as IDBOpenDBRequest).transaction!;

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

      // v1/v2 → v3: rebuild cards store with composite-id keyPath, spawn
      // recall sibling for every existing card (active, due now). The v3
      // rebuild also subsumes the legacy v1→v2 "fill missing status" step:
      // we tolerate a missing `status` field here and default to active.
      if (e.oldVersion >= 1 && e.oldVersion < 3) {
        const oldStore = upgradeTx.objectStore("cards");
        const collected: Array<Record<string, unknown>> = [];
        const cursorReq = oldStore.openCursor();
        cursorReq.onsuccess = (ev) => {
          const cursor = (ev.target as IDBRequest<IDBCursorWithValue>).result;
          if (cursor) {
            collected.push(cursor.value);
            cursor.continue();
            return;
          }
          // Cursor exhausted — recreate the store with the new schema.
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
            const recognition: SrsCard = {
              id: cardId(word, "recognition"),
              word,
              direction: "recognition",
              due_ms: num(old.due_ms, now),
              interval_days: num(old.interval_days, 0),
              // ease_factor must be > 0; 0/missing → SM-2 default.
              ease_factor: num(old.ease_factor, 2.5) || 2.5,
              repetitions: num(old.repetitions, 0),
              added_ms: num(old.added_ms, now),
              status: (old.status as SrsCard["status"]) ?? "active",
            };
            newStore.add(recognition);
            const recall: SrsCard = {
              id: cardId(word, "recall"),
              word,
              direction: "recall",
              due_ms: now,
              interval_days: 0,
              ease_factor: 2.5,
              repetitions: 0,
              added_ms: recognition.added_ms,
              status: "active",
            };
            newStore.add(recall);
          }
        };
      }
    };

    req.onsuccess = (e) => {
      const opened = (e.target as IDBOpenDBRequest).result;
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
    req.onerror = () => reject(req.error);
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

/** Deletes both direction siblings for a word. */
export async function deleteCard(word: string): Promise<void> {
  const database = await openDb();
  return new Promise((resolve, reject) => {
    const t = database.transaction("cards", "readwrite");
    const store = t.objectStore("cards");
    store.delete(cardId(word, "recognition"));
    store.delete(cardId(word, "recall"));
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

/** Deletes a single sibling by composite id (used when one direction graduates). */
export function deleteCardById(id: string): Promise<undefined> {
  return tx("cards", "readwrite", (s) => s.delete(id));
}

export function addLookupHistory(
  word: string,
  reading: string,
): Promise<IDBValidKey> {
  return tx("lookup_history", "readwrite", (s) =>
    s.add({ word, reading, ts: Date.now() }),
  );
}
