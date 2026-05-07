import type { SrsCard } from "../shared/types.ts";

const DB_NAME = "yomeru-db";
const DB_VERSION = 2;

let db: IDBDatabase | null = null;

export async function openDb(): Promise<IDBDatabase> {
  if (db) return db;
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);

    req.onupgradeneeded = (e) => {
      const database = (e.target as IDBOpenDBRequest).result;
      const upgradeTx = (e.target as IDBOpenDBRequest).transaction!;

      if (e.oldVersion < 1) {
        const cards = database.createObjectStore("cards", { keyPath: "word" });
        cards.createIndex("due_ms", "due_ms", { unique: false });
        cards.createIndex("added_ms", "added_ms", { unique: false });
        cards.createIndex("status", "status", { unique: false });

        const history = database.createObjectStore("lookup_history", {
          keyPath: "id",
          autoIncrement: true,
        });
        history.createIndex("word", "word", { unique: false });
        history.createIndex("ts", "ts", { unique: false });
      }

      if (e.oldVersion >= 1 && e.oldVersion < 2) {
        const cards = upgradeTx.objectStore("cards");
        cards.createIndex("status", "status", { unique: false });
        const cursorReq = cards.openCursor();
        cursorReq.onsuccess = (ev) => {
          const cursor = (ev.target as IDBRequest<IDBCursorWithValue>).result;
          if (!cursor) return;
          const card = cursor.value;
          if (!card.status) {
            card.status = "active";
            cursor.update(card);
          }
          cursor.continue();
        };
      }
    };

    req.onsuccess = (e) => {
      db = (e.target as IDBOpenDBRequest).result;
      resolve(db);
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
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      }),
  );
}

export function putCard(card: SrsCard): Promise<IDBValidKey> {
  return tx("cards", "readwrite", (s) => s.put(card));
}

export async function getCard(word: string): Promise<SrsCard | null> {
  return tx<SrsCard | undefined>("cards", "readonly", (s) => s.get(word)).then(
    (r) => r ?? null,
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

export async function promoteCard(word: string): Promise<void> {
  const card = await getCard(word);
  if (!card) return;
  card.status = "active";
  await putCard(card);
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
      const card = cursor.value;
      card.status = "active";
      cursor.update(card);
      cursor.continue();
    };
    req.onerror = () => reject(req.error);
  });
}

export function deleteCard(word: string): Promise<undefined> {
  return tx("cards", "readwrite", (s) => s.delete(word));
}

export function addLookupHistory(
  word: string,
  reading: string,
): Promise<IDBValidKey> {
  return tx("lookup_history", "readwrite", (s) =>
    s.add({ word, reading, ts: Date.now() }),
  );
}
