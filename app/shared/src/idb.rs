//! IndexedDB wrapper for cards. Mirrors `extension/src/background/idb.ts` at
//! schema v4 (composite-id cards, FSRS-shaped fields, recognition/recall
//! siblings). The website starts at v4 — no legacy migrations needed.

use idb::{
    Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams, Query,
    TransactionMode,
};
use wasm_bindgen::JsValue;

use crate::types::{card_id, CardDirection, CardStatus, SrsCard};

const DB_NAME: &str = "yomeru-db";
const DB_VERSION: u32 = 4;
const STORE: &str = "cards";

async fn open() -> Result<Database, idb::Error> {
    let factory = Factory::new()?;
    let mut req = factory.open(DB_NAME, Some(DB_VERSION))?;
    req.on_upgrade_needed(|event| {
        let db = event.database().expect("upgrade event has db");
        if db.store_names().iter().any(|n| n == STORE) {
            return;
        }
        let mut params = ObjectStoreParams::new();
        params.key_path(Some(KeyPath::new_single("id")));
        let store = db.create_object_store(STORE, params).expect("create store");

        let mut idx = IndexParams::new();
        idx.unique(false);
        store
            .create_index("due_ms", KeyPath::new_single("due_ms"), Some(idx.clone()))
            .ok();
        store
            .create_index("added_ms", KeyPath::new_single("added_ms"), Some(idx.clone()))
            .ok();
        store
            .create_index("status", KeyPath::new_single("status"), Some(idx.clone()))
            .ok();
        store
            .create_index("word", KeyPath::new_single("word"), Some(idx))
            .ok();
    });
    req.await
}

fn to_value(card: &SrsCard) -> Result<JsValue, serde_wasm_bindgen::Error> {
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(card, &ser)
}

fn from_value(v: JsValue) -> Result<SrsCard, serde_wasm_bindgen::Error> {
    serde_wasm_bindgen::from_value(v)
}

async fn run_rw<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&idb::ObjectStore) -> Result<R, String>,
{
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let out = f(&store)?;
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(out)
}

pub async fn put_card(card: &SrsCard) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let val = to_value(card).map_err(|e| e.to_string())?;
    store
        .put(&val, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn put_cards(cards: &[SrsCard]) -> Result<(), String> {
    if cards.is_empty() {
        return Ok(());
    }
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    for c in cards {
        let val = to_value(c).map_err(|e| e.to_string())?;
        store
            .put(&val, None)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_card(word: &str, direction: CardDirection) -> Result<Option<SrsCard>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let key = JsValue::from_str(&card_id(word, direction));
    let v = store
        .get(Query::Key(key))
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(v.and_then(|val| from_value(val).ok()))
}

pub async fn get_cards_by_word(word: &str) -> Result<Vec<SrsCard>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let index = store.index("word").map_err(|e| e.to_string())?;
    let key = JsValue::from_str(word);
    let arr = index
        .get_all(Some(Query::Key(key)), None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(arr.into_iter().filter_map(|v| from_value(v).ok()).collect())
}

pub async fn get_all_cards() -> Result<Vec<SrsCard>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let arr = store
        .get_all(None, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(arr.into_iter().filter_map(|v| from_value(v).ok()).collect())
}

pub async fn get_due_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| matches!(c.status, CardStatus::Active) && c.due_ms <= now_ms);
    all.sort_by(|a, b| a.due_ms.partial_cmp(&b.due_ms).unwrap_or(std::cmp::Ordering::Equal));
    Ok(all)
}

pub async fn get_staging_cards() -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| matches!(c.status, CardStatus::Staging));
    all.sort_by(|a, b| a.added_ms.partial_cmp(&b.added_ms).unwrap_or(std::cmp::Ordering::Equal));
    Ok(all)
}

pub async fn promote_card(word: &str) -> Result<(), String> {
    let siblings = get_cards_by_word(word).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Staging))
        .map(|mut c| {
            c.status = CardStatus::Active;
            c
        })
        .collect();
    put_cards(&to_put).await
}

pub async fn delete_card(word: &str) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    for d in [CardDirection::Recognition, CardDirection::Recall] {
        let key = JsValue::from_str(&card_id(word, d));
        store
            .delete(Query::Key(key))
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn delete_card_by_id(id: &str) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let key = JsValue::from_str(id);
    store
        .delete(Query::Key(key))
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(dead_code)]
async fn _unused_run_rw() {
    let _ = run_rw::<_, ()>(|_| Ok(())).await;
}
