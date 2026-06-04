//! IndexedDB wrapper for cards. Mirrors `extension/src/background/idb.ts` at
//! schema v6 (cards keyed on JMdict ent_seq + tombstones store for sync).
//! v6 replaces the old `word` secondary index with `sequence`; users carrying
//! v5 data are expected to export and re-import.

use log::error;
use idb::{
    Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams, Query,
    TransactionMode,
};
use wasm_bindgen::JsValue;

use crate::types::{CardDirection, CardStatus, SrsCard, card_id};

const DB_NAME: &str = "yomeru-db";
// v7 re-runs the v6 sequence-keyed reset to self-heal any DB left half-migrated
// by an earlier build that created the store without the `sequence` index.
const DB_VERSION: u32 = 7;
const STORE: &str = "cards";
const TOMB_STORE: &str = "tombstones";

async fn open() -> Result<Database, idb::Error> {
    let factory = Factory::new()?;
    let mut req = factory.open(DB_NAME, Some(DB_VERSION))?;
    req.on_upgrade_needed(|event| {
        // The callback is sync — we can't propagate Result. Log and bail on
        // any setup failure; subsequent transactions on a half-set-up store
        // will surface the failure to the caller as a normal idb error.
        let old_version = event.old_version().unwrap_or(0);
        let db = match event.database() {
            Ok(db) => db,
            Err(e) => {
                error!("idb upgrade: event.database() failed: {e:?}");
                return;
            }
        };
        // v6/v7 clean break: cards used to carry a `word` secondary index and
        // key on a surface string; they now key on JMdict `sequence`. Drop the
        // pre-v7 stores so they're recreated below on the sequence-keyed
        // schema. Mirrors extension/src/background/idb.ts. Scoped to
        // old_version < 7 so a later bump can't wipe good data. This MUST match
        // the TS side: whichever layer opens yomeru-db first runs the upgrade,
        // and the other then relies on the `sequence` index existing.
        if (1..7).contains(&old_version) {
            if db.store_names().iter().any(|n| n == STORE) {
                if let Err(e) = db.delete_object_store(STORE) {
                    error!("idb upgrade: delete_object_store({STORE}) failed: {e:?}");
                }
            }
            if db.store_names().iter().any(|n| n == TOMB_STORE) {
                if let Err(e) = db.delete_object_store(TOMB_STORE) {
                    error!("idb upgrade: delete_object_store({TOMB_STORE}) failed: {e:?}");
                }
            }
        }
        if !db.store_names().iter().any(|n| n == STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_single("id")));
            let store = match db.create_object_store(STORE, params) {
                Ok(s) => s,
                Err(e) => {
                    error!("idb upgrade: create_object_store({STORE}) failed: {e:?}");
                    return;
                }
            };

            let mut idx = IndexParams::new();
            idx.unique(false);
            store
                .create_index("due_ms", KeyPath::new_single("due_ms"), Some(idx.clone()))
                .ok();
            store
                .create_index(
                    "added_ms",
                    KeyPath::new_single("added_ms"),
                    Some(idx.clone()),
                )
                .ok();
            store
                .create_index("status", KeyPath::new_single("status"), Some(idx.clone()))
                .ok();
            store
                .create_index("sequence", KeyPath::new_single("sequence"), Some(idx))
                .ok();
        }
        if !db.store_names().iter().any(|n| n == TOMB_STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_single("id")));
            if let Err(e) = db.create_object_store(TOMB_STORE, params) {
                error!("idb upgrade: create_object_store({TOMB_STORE}) failed: {e:?}");
            }
        }
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

pub async fn get_card(sequence: u32, direction: CardDirection) -> Result<Option<SrsCard>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let key = JsValue::from_str(&card_id(sequence, direction));
    let v = store
        .get(Query::Key(key))
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(v.and_then(|val| from_value(val).ok()))
}

pub async fn get_cards_by_sequence(sequence: u32) -> Result<Vec<SrsCard>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let index = store.index("sequence").map_err(|e| e.to_string())?;
    let key = JsValue::from_f64(sequence as f64);
    let arr = index
        .get_all(Some(Query::Key(key)), None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(arr.into_iter().filter_map(|v| from_value(v).ok()).collect())
}

pub async fn has_card(sequence: u32) -> Result<bool, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let index = store.index("sequence").map_err(|e| e.to_string())?;
    let key = JsValue::from_f64(sequence as f64);
    let has_key = index
        .get_key(Query::Key(key))
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;

    Ok(has_key.is_some())
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
    all.sort_by(|a, b| {
        a.due_ms
            .partial_cmp(&b.due_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(all)
}

pub async fn get_staging_cards() -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| matches!(c.status, CardStatus::Staging));
    all.sort_by(|a, b| {
        a.added_ms
            .partial_cmp(&b.added_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(all)
}

pub async fn promote_card(sequence: u32) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
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

pub async fn delete_card(sequence: u32) -> Result<(), String> {
    let ids = [
        card_id(sequence, CardDirection::Recognition),
        card_id(sequence, CardDirection::Recall),
    ];
    delete_ids_with_tombstones(&ids).await
}

pub async fn delete_card_by_id(id: &str) -> Result<(), String> {
    delete_ids_with_tombstones(std::slice::from_ref(&id.to_string())).await
}

/// Atomically deletes the given card ids and writes tombstones for each, in
/// a single transaction across both stores so a crash mid-delete can't lose
/// the tombstone (which would cause the next sync to resurrect the card).
async fn delete_ids_with_tombstones(ids: &[String]) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE, TOMB_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let cards = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let tombs = tx.object_store(TOMB_STORE).map_err(|e| e.to_string())?;
    let now = js_sys::Date::now();
    for id in ids {
        let tomb_val = serde_wasm_bindgen::to_value(&serde_json::json!({
            "id": id,
            "deleted_at": now,
        }))
        .map_err(|e| e.to_string())?;
        tombs
            .put(&tomb_val, None)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
        cards
            .delete(Query::Key(JsValue::from_str(id)))
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

pub async fn get_all_tombstones() -> Result<Vec<String>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[TOMB_STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(TOMB_STORE).map_err(|e| e.to_string())?;
    let arr = store
        .get_all(None, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    let ids = arr
        .into_iter()
        .filter_map(|v| serde_wasm_bindgen::from_value::<serde_json::Value>(v).ok())
        .filter_map(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .collect();
    Ok(ids)
}

pub async fn clear_tombstones(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[TOMB_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(TOMB_STORE).map_err(|e| e.to_string())?;
    for id in ids {
        store
            .delete(Query::Key(JsValue::from_str(id)))
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

/// Applies tombstones that came from the server. Deletes the matching card
/// rows without writing local tombstones — the server is already
/// authoritative for these ids. Use for remote-driven deletes; user-driven
/// deletes go through [`delete_card`] / [`delete_card_by_id`].
pub async fn apply_remote_deletions(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    for id in ids {
        store
            .delete(Query::Key(JsValue::from_str(id)))
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

#[allow(dead_code)]
async fn _unused_run_rw() {
    let _ = run_rw::<_, ()>(|_| Ok(())).await;
}
