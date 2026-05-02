const DB_NAME = "japanese-reader-db";
const DB_VERSION = 1;
let db = null;
async function openDb() {
  if (db) return db;
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = (e) => {
      const database = e.target.result;
      if (!database.objectStoreNames.contains("cards")) {
        const cards = database.createObjectStore("cards", { keyPath: "word" });
        cards.createIndex("due_ms", "due_ms", { unique: false });
        cards.createIndex("added_ms", "added_ms", { unique: false });
      }
      if (!database.objectStoreNames.contains("lookup_history")) {
        const history = database.createObjectStore("lookup_history", { keyPath: "id", autoIncrement: true });
        history.createIndex("word", "word", { unique: false });
        history.createIndex("ts", "ts", { unique: false });
      }
    };
    req.onsuccess = (e) => {
      db = e.target.result;
      resolve(db);
    };
    req.onerror = () => reject(req.error);
  });
}
function tx(store, mode, fn) {
  return openDb().then(
    (database) => new Promise((resolve, reject) => {
      const t = database.transaction(store, mode);
      const req = fn(t.objectStore(store));
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    })
  );
}
function putCard(card) {
  return tx("cards", "readwrite", (s) => s.put(card));
}
function getCard(word) {
  return tx("cards", "readonly", (s) => s.get(word)).then((r) => r ?? null);
}
function getAllCards() {
  return tx("cards", "readonly", (s) => s.getAll());
}
function getDueCards(nowMs) {
  return openDb().then(
    (database) => new Promise((resolve, reject) => {
      const req = database.transaction("cards", "readonly").objectStore("cards").index("due_ms").getAll(IDBKeyRange.upperBound(nowMs));
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    })
  );
}
function deleteCard(word) {
  return tx("cards", "readwrite", (s) => s.delete(word));
}
function addLookupHistory(word, reading) {
  return tx("lookup_history", "readwrite", (s) => s.add({ word, reading, ts: Date.now() }));
}
let srs = null;
async function initSrs() {
  const jsUrl = browser.runtime.getURL("_generated/srs-wasm/srs_wasm.js");
  const binUrl = browser.runtime.getURL("_generated/srs-wasm/srs_wasm_bg.wasm");
  const mod = await import(
    /* @vite-ignore */
    jsUrl
  );
  await mod.default(binUrl);
  srs = new mod.SrsEngine();
}
async function ensureSrs() {
  if (!srs) await initSrs();
}
initSrs();
browser.runtime.onMessage.addListener((msg) => {
  switch (msg.type) {
    case "ADD_WORD":
      return handleAddWord(msg.payload);
    case "REVIEW_CARD":
      return handleReviewCard(msg.payload);
    case "GET_DUE":
      return handleGetDue();
    case "GET_ALL_CARDS":
      return handleGetAllCards();
    case "DELETE_CARD":
      return handleDeleteCard(msg.payload);
    case "LOG_LOOKUP":
      return handleLogLookup(msg.payload);
    default:
      return Promise.resolve({ error: "Unknown message type" });
  }
});
async function handleAddWord({ word, reading, meaning_en }) {
  await ensureSrs();
  if (await getCard(word)) return { success: true, existing: true };
  const card = srs.new_card(word, reading, meaning_en ?? "", Date.now());
  await putCard(card);
  return { success: true, existing: false };
}
async function handleReviewCard({ word, rating }) {
  await ensureSrs();
  const card = await getCard(word);
  if (!card) return { error: "Card not found" };
  const updated = srs.review_card(card, rating, Date.now());
  await putCard(updated);
  return { success: true, card: updated };
}
async function handleGetDue() {
  return { cards: await getDueCards(Date.now()) };
}
async function handleGetAllCards() {
  return { cards: await getAllCards() };
}
async function handleDeleteCard({ word }) {
  await deleteCard(word);
  return { success: true };
}
async function handleLogLookup({ word, reading }) {
  await addLookupHistory(word, reading);
  return { success: true };
}
