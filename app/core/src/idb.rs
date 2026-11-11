//! IndexedDB wrapper for cards. Mirrors `extension/src/background/idb.ts` at
//! schema v9 (cards keyed on JMdict ent_seq, a tombstones store, a review
//! outbox, and the delta-sync dirty set and cursor). v6 replaced the old `word` secondary index with
//! `sequence`; users carrying v5 data are expected to export and re-import.

pub use crate::store_common::*;
use crate::sync::SyncCursor;
use crate::types::{CardDirection, ReviewLog, SrsCard, card_id};
use idb::{
    Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams, Query,
    TransactionMode,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::JsValue;

const DB_NAME: &str = "yomeru-db";
// v7 re-runs the v6 sequence-keyed reset to self-heal any DB left half-migrated
// by an earlier build that created the store without the `sequence` index.
// v8 adds the review outbox. v9 adds the delta-sync dirty set and the meta
// store holding the sync cursor. The extension opens the same database, so
// this and `DB_VERSION` in idb.ts must move together.
const DB_VERSION: u32 = 9;
const STORE: &str = "cards";
const TOMB_STORE: &str = "tombstones";
const REVIEW_STORE: &str = "review_outbox";
/// The server's per-card drill counts as of the last sync, `{card_id, count}`.
/// Written by whichever sync loop runs (Rust here, `sync.ts` in the
/// extension), so both builds read the counts the same way.
const DRILL_STORE: &str = "drill_counts";
/// Cards changed locally since the last successful sync, `{id, v}` where `v`
/// is the `updated_ms` the change was stamped with. Written in the same
/// transaction as the card, so a change can't land without being queued.
const DIRTY_STORE: &str = "dirty";
/// Out-of-line keyed values that must live and die with the card data.
const META_STORE: &str = "meta";
const CURSOR_KEY: &str = "sync_cursor";

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
                tracing::error!("idb upgrade: event.database() failed: {e:?}");
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
            if db.store_names().iter().any(|n| n == STORE)
                && let Err(e) = db.delete_object_store(STORE)
            {
                tracing::error!("idb upgrade: delete_object_store({STORE}) failed: {e:?}");
            }
            if db.store_names().iter().any(|n| n == TOMB_STORE)
                && let Err(e) = db.delete_object_store(TOMB_STORE)
            {
                tracing::error!("idb upgrade: delete_object_store({TOMB_STORE}) failed: {e:?}");
            }
        }
        if !db.store_names().iter().any(|n| n == STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_single("id")));
            let store = match db.create_object_store(STORE, params) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("idb upgrade: create_object_store({STORE}) failed: {e:?}");
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
                tracing::error!("idb upgrade: create_object_store({TOMB_STORE}) failed: {e:?}");
            }
        }
        if !db.store_names().iter().any(|n| n == REVIEW_STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_array(["card_id", "reviewed_ms"])));
            if let Err(e) = db.create_object_store(REVIEW_STORE, params) {
                tracing::error!("idb upgrade: create_object_store({REVIEW_STORE}) failed: {e:?}");
            }
        }
        if !db.store_names().iter().any(|n| n == DRILL_STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_single("card_id")));
            if let Err(e) = db.create_object_store(DRILL_STORE, params) {
                tracing::error!("idb upgrade: create_object_store({DRILL_STORE}) failed: {e:?}");
            }
        }
        // → v9: delta sync. Additive; with no cursor stored yet, the first sync
        // after the upgrade is a full one, so the dirty set can start empty.
        if !db.store_names().iter().any(|n| n == DIRTY_STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_single("id")));
            if let Err(e) = db.create_object_store(DIRTY_STORE, params) {
                tracing::error!("idb upgrade: create_object_store({DIRTY_STORE}) failed: {e:?}");
            }
        }
        if !db.store_names().iter().any(|n| n == META_STORE)
            && let Err(e) = db.create_object_store(META_STORE, ObjectStoreParams::new())
        {
            tracing::error!("idb upgrade: create_object_store({META_STORE}) failed: {e:?}");
        }
    });
    req.await
}

fn to_value<S: Serialize>(card: &S) -> Result<JsValue, serde_wasm_bindgen::Error> {
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(card, &ser)
}

fn from_value<D: DeserializeOwned>(v: JsValue) -> Result<D, serde_wasm_bindgen::Error> {
    serde_wasm_bindgen::from_value(v)
}

/// Writes `cards`, optionally stamping each with a fresh `updated_ms`.
///
/// `stamp` is the whole reason the card writes funnel through here: every local
/// mutation has to advance the sync merge key, and doing it at the single write
/// point means review, promotion, reset, priority and import all get it without
/// remembering to. Sync writes pass `false` — re-stamping cards the server just
/// sent would make them look locally-modified and ping-pong between devices.
/// A stamped write is also queued in the dirty set for the next delta sync,
/// in the same transaction.
pub(crate) async fn put_all(cards: &[SrsCard], stamp: bool) -> Result<(), String> {
    if cards.is_empty() {
        return Ok(());
    }
    let now = crate::rt::now_ms();
    let db = open().await.map_err(|e| e.to_string())?;
    let stores: &[&str] = if stamp {
        &[STORE, DIRTY_STORE]
    } else {
        &[STORE]
    };
    let tx = db
        .transaction(stores, TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let dirty = if stamp {
        Some(tx.object_store(DIRTY_STORE).map_err(|e| e.to_string())?)
    } else {
        None
    };
    for c in cards {
        let val = if stamp {
            let mut c = c.clone();
            c.updated_ms = now;
            to_value(&c).map_err(|e| e.to_string())?
        } else {
            to_value(c).map_err(|e| e.to_string())?
        };
        store
            .put(&val, None)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
        if let Some(dirty) = &dirty {
            let entry = DirtyEntry {
                id: c.id.clone(),
                v: now,
            };
            dirty
                .put(&to_value(&entry).map_err(|e| e.to_string())?, None)
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())?;
        }
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
    let key = JsValue::from_f64(f64::from(sequence));
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
    let key = JsValue::from_f64(f64::from(sequence));
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
    Ok(arr
        .into_iter()
        .filter_map(|v| from_value::<SrsCard>(v).ok())
        // Cards stored before `updated_ms` existed deserialize with 0. Resolve
        // that here so callers — the sync upload above all — always hand out a
        // real merge version rather than one that loses every comparison.
        .map(|mut c| {
            c.updated_ms = c.version_ms();
            c
        })
        .collect())
}

/// Atomically deletes the given card ids and writes tombstones for each, in
/// a single transaction across both stores so a crash mid-delete can't lose
/// the tombstone (which would cause the next sync to resurrect the card).
pub(crate) async fn delete_ids_with_tombstones(ids: &[String]) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE, TOMB_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let cards = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let tombs = tx.object_store(TOMB_STORE).map_err(|e| e.to_string())?;
    let now = crate::rt::now_ms();
    for id in ids {
        let tomb_val = to_value(&Tombstone {
            deleted_at: now,
            id: id.to_string(),
        })
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

pub async fn get_all_tombstones() -> Result<Vec<Tombstone>, String> {
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
    Ok(arr
        .into_iter()
        .filter_map(|v| from_value::<Tombstone>(v).ok())
        .filter(|t| !t.id.is_empty())
        .collect())
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

/// Saves a reviewed card and queues its answer for the next sync, in one
/// transaction: the log can't record a review that didn't apply, nor miss one
/// that did. Both requests are issued before anything is awaited, so even if
/// the calling task is cancelled mid-way, IndexedDB commits the pair on its own
/// once they complete. Stamps `updated_ms` like [`put_card`].
pub async fn put_card_logged(card: &SrsCard, row: &ReviewLog) -> Result<(), String> {
    let mut card = card.clone();
    card.updated_ms = crate::rt::now_ms();
    let card_val = to_value(&card).map_err(|e| e.to_string())?;
    let dirty_val = to_value(&DirtyEntry {
        id: card.id.clone(),
        v: card.updated_ms,
    })
    .map_err(|e| e.to_string())?;
    let row_val = to_value(row).map_err(|e| e.to_string())?;
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(
            &[STORE, DIRTY_STORE, REVIEW_STORE],
            TransactionMode::ReadWrite,
        )
        .map_err(|e| e.to_string())?;
    let cards = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let dirty = tx.object_store(DIRTY_STORE).map_err(|e| e.to_string())?;
    let reviews = tx.object_store(REVIEW_STORE).map_err(|e| e.to_string())?;
    let put_card = cards.put(&card_val, None).map_err(|e| e.to_string())?;
    let put_dirty = dirty.put(&dirty_val, None).map_err(|e| e.to_string())?;
    let put_row = reviews.put(&row_val, None).map_err(|e| e.to_string())?;
    put_card.await.map_err(|e| e.to_string())?;
    put_dirty.await.map_err(|e| e.to_string())?;
    put_row.await.map_err(|e| e.to_string())?;
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Queues one answer for the next sync.
pub async fn add_review(row: &ReviewLog) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[REVIEW_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(REVIEW_STORE).map_err(|e| e.to_string())?;
    store
        .put(&to_value(row).map_err(|e| e.to_string())?, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Every answer not yet acknowledged by the server.
pub async fn get_outbox() -> Result<Vec<ReviewLog>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[REVIEW_STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(REVIEW_STORE).map_err(|e| e.to_string())?;
    let arr = store
        .get_all(None, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(arr
        .into_iter()
        .filter_map(|v| from_value::<ReviewLog>(v).ok())
        .collect())
}

#[derive(Serialize, Deserialize)]
struct DrillCount {
    card_id: String,
    count: u32,
}

/// What drill rotation needs: the server's per-card drill counts from the last
/// sync, and the rows still waiting to be uploaded. Read in one transaction so
/// a sync settling in between can't make a row look both counted and pending.
pub async fn get_drill_state()
-> Result<(std::collections::HashMap<String, u32>, Vec<ReviewLog>), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[DRILL_STORE, REVIEW_STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let counts = tx
        .object_store(DRILL_STORE)
        .map_err(|e| e.to_string())?
        .get_all(None, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    let outbox = tx
        .object_store(REVIEW_STORE)
        .map_err(|e| e.to_string())?
        .get_all(None, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    let counts = counts
        .into_iter()
        .filter_map(|v| from_value::<DrillCount>(v).ok())
        .map(|d| (d.card_id, d.count))
        .collect();
    let outbox = outbox
        .into_iter()
        .filter_map(|v| from_value::<ReviewLog>(v).ok())
        .collect();
    Ok((counts, outbox))
}

/// Settles an upload the server acknowledged: stores the server's drill counts
/// (which already include the sent rows) and drops exactly the rows that were
/// sent. Rows logged while the request was in flight have other keys and stay
/// queued. One transaction, so the counts and the outbox never disagree about
/// whether a row was counted. Mirrors `settleOutbox` in
/// `extension/src/background/idb.ts`.
///
/// `replace`: the counts are the user's complete set (a full sync), so the
/// cache is cleared first. Otherwise they cover only the cards whose count
/// changed (a delta) and are merged over the cache.
pub async fn settle_outbox(
    sent: &[ReviewLog],
    counts: &std::collections::HashMap<String, u32>,
    replace: bool,
) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[REVIEW_STORE, DRILL_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let outbox = tx.object_store(REVIEW_STORE).map_err(|e| e.to_string())?;
    for r in sent {
        let key = js_sys::Array::of2(
            &JsValue::from_str(&r.card_id),
            &JsValue::from_f64(r.reviewed_ms),
        );
        outbox
            .delete(Query::Key(key.into()))
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
    }
    let store = tx.object_store(DRILL_STORE).map_err(|e| e.to_string())?;
    if replace {
        store
            .clear()
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
    }
    for (card_id, count) in counts {
        let row = DrillCount {
            card_id: card_id.clone(),
            count: *count,
        };
        store
            .put(&to_value(&row).map_err(|e| e.to_string())?, None)
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

// ── Delta sync state ─────────────────────────────────────────────────

/// The dirty set as it stands, to upload and later hand to [`clear_dirty`].
pub async fn get_dirty() -> Result<Vec<DirtyEntry>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[DIRTY_STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let arr = tx
        .object_store(DIRTY_STORE)
        .map_err(|e| e.to_string())?
        .get_all(None, None)
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(arr
        .into_iter()
        .filter_map(|v| from_value::<DirtyEntry>(v).ok())
        .collect())
}

/// The cards behind a dirty snapshot. Ids deleted since are skipped.
pub async fn get_cards_by_ids(ids: &[String]) -> Result<Vec<SrsCard>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let v = store
            .get(Query::Key(JsValue::from_str(id)))
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
        if let Some(c) = v.and_then(|v| from_value::<SrsCard>(v).ok()) {
            out.push(c);
        }
    }
    Ok(out)
}

/// Drops the entries of an uploaded snapshot, except any re-stamped since it
/// was taken: that card changed while the sync was in flight, and its new
/// state still has to go up.
pub async fn clear_dirty(snapshot: &[DirtyEntry]) -> Result<(), String> {
    if snapshot.is_empty() {
        return Ok(());
    }
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[DIRTY_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(DIRTY_STORE).map_err(|e| e.to_string())?;
    for e in snapshot {
        let key = JsValue::from_str(&e.id);
        let cur = store
            .get(Query::Key(key.clone()))
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?
            .and_then(|v| from_value::<DirtyEntry>(v).ok());
        if cur.is_some_and(|c| c.v == e.v) {
            store
                .delete(Query::Key(key))
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Where this device's view of the server ends. Kept in IndexedDB beside the
/// cards on purpose: if the database is wiped, the cursor goes with it and the
/// next sync is a full one. A cursor that outlived its data would ask for a
/// delta and never get the lost cards back.
pub async fn get_sync_cursor() -> Result<Option<SyncCursor>, String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[META_STORE], TransactionMode::ReadOnly)
        .map_err(|e| e.to_string())?;
    let v = tx
        .object_store(META_STORE)
        .map_err(|e| e.to_string())?
        .get(Query::Key(JsValue::from_str(CURSOR_KEY)))
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(v.and_then(|v| from_value::<SyncCursor>(v).ok()))
}

/// Stores the cursor, or forgets it with `None` so the next sync is full.
pub async fn set_sync_cursor(cursor: Option<&SyncCursor>) -> Result<(), String> {
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[META_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(META_STORE).map_err(|e| e.to_string())?;
    let key = JsValue::from_str(CURSOR_KEY);
    match cursor {
        Some(c) => {
            store
                .put(&to_value(c).map_err(|e| e.to_string())?, Some(&key))
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())?;
        }
        None => {
            store
                .delete(Query::Key(key))
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
