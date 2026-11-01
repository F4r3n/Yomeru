//! IndexedDB wrapper for cards. Mirrors `extension/src/background/idb.ts` at
//! schema v8 (cards keyed on JMdict ent_seq, a tombstones store and a review
//! outbox for sync). v6 replaced the old `word` secondary index with
//! `sequence`; users carrying v5 data are expected to export and re-import.

use crate::types::{CardDirection, CardStatus, ReviewLog, SrsCard, card_id};
use dioxus::logger::tracing;
use idb::{
    Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams, Query,
    TransactionMode,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::JsValue;

const DB_NAME: &str = "yomeru-db";
// v7 re-runs the v6 sequence-keyed reset to self-heal any DB left half-migrated
// by an earlier build that created the store without the `sequence` index.
// v8 adds the review outbox. The extension opens the same database, so this
// and `DB_VERSION` in idb.ts must move together.
const DB_VERSION: u32 = 8;
const STORE: &str = "cards";
const TOMB_STORE: &str = "tombstones";
const REVIEW_STORE: &str = "review_outbox";
/// The server's per-card drill counts as of the last sync, `{card_id, count}`.
/// Replaced wholesale by whichever sync loop runs (Rust here, `sync.ts` in the
/// extension), so both builds read the counts the same way.
const DRILL_STORE: &str = "drill_counts";

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
                dioxus::logger::tracing::error!("idb upgrade: event.database() failed: {e:?}");
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

/// Writes `cards`, optionally stamping each with a fresh `updated_ms`.
///
/// `stamp` is the whole reason the card writes funnel through here: every local
/// mutation has to advance the sync merge key, and doing it at the single write
/// point means review, promotion, reset, priority and import all get it without
/// remembering to. Sync writes pass `false` — re-stamping cards the server just
/// sent would make them look locally-modified and ping-pong between devices.
async fn put_all(cards: &[SrsCard], stamp: bool) -> Result<(), String> {
    if cards.is_empty() {
        return Ok(());
    }
    let now = js_sys::Date::now();
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let store = tx.object_store(STORE).map_err(|e| e.to_string())?;
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
    }
    tx.commit()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn put_card(card: &SrsCard) -> Result<(), String> {
    put_all(std::slice::from_ref(card), true).await
}

pub async fn put_cards(cards: &[SrsCard]) -> Result<(), String> {
    put_all(cards, true).await
}

/// Writes cards adopted from the server verbatim, keeping the `updated_ms` they
/// arrived with. Only the sync merge should use this — see [`put_all`].
pub async fn put_cards_synced(cards: &[SrsCard]) -> Result<(), String> {
    put_all(cards, false).await
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

/// Due word cards, ordered by [`crate::srs::order_due`]. Kanji cards have their own session — see
/// [`get_due_kanji_cards`].
pub async fn get_due_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, false).await
}

/// Due kanji cards, ordered by [`crate::srs::order_due`].
pub async fn get_due_kanji_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, true).await
}

async fn get_due(now_ms: f64, kanji: bool) -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| {
        c.is_kanji() == kanji && matches!(c.status, CardStatus::Active) && c.due_ms <= now_ms
    });
    crate::srs::order_due(&mut all);
    Ok(all)
}

pub async fn get_staging_cards() -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| matches!(c.status, CardStatus::Staging));
    all.sort_by(|a, b| {
        b.priority.cmp(&a.priority).then_with(|| {
            b.added_ms
                .partial_cmp(&a.added_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    Ok(all)
}

/// Bumps the priority score of every staging sibling of `sequence` by 1, up
/// to [`crate::types::MAX_PRIORITY`]. Called when the user re-clicks "Add"
/// on an already-staged word.
pub async fn bump_priority(sequence: u32) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Staging))
        .map(|mut c| {
            c.priority = c.priority.saturating_add(1).min(crate::types::MAX_PRIORITY);
            c
        })
        .collect();
    put_cards(&to_put).await
}

/// Resets every active or graduated sibling of `sequence` back to a fresh
/// "New" FSRS state in the active word list — the mechanism by which a
/// graduated card can re-enter Review. Destructive.
pub async fn reset_card(sequence: u32, now_ms: f64) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Active | CardStatus::Graduated))
        .map(|mut c| {
            c.reset_progression(now_ms);
            c.status = CardStatus::Active;
            c
        })
        .collect();
    put_cards(&to_put).await
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

/// A local record that the user deleted a card, and when. The time travels to
/// the server on the next sync so a delete made while offline is ordered by
/// when it actually happened rather than when it was finally uploaded.
#[derive(Serialize, Deserialize, Clone)]
pub struct Tombstone {
    pub id: String,
    pub deleted_at: f64,
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
    card.updated_ms = js_sys::Date::now();
    let card_val = to_value(&card).map_err(|e| e.to_string())?;
    let row_val = to_value(row).map_err(|e| e.to_string())?;
    let db = open().await.map_err(|e| e.to_string())?;
    let tx = db
        .transaction(&[STORE, REVIEW_STORE], TransactionMode::ReadWrite)
        .map_err(|e| e.to_string())?;
    let cards = tx.object_store(STORE).map_err(|e| e.to_string())?;
    let reviews = tx.object_store(REVIEW_STORE).map_err(|e| e.to_string())?;
    let put_card = cards.put(&card_val, None).map_err(|e| e.to_string())?;
    let put_row = reviews.put(&row_val, None).map_err(|e| e.to_string())?;
    put_card.await.map_err(|e| e.to_string())?;
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

/// Settles an upload the server acknowledged: replaces the cached drill counts
/// with the server's (which already include the sent rows) and drops exactly
/// the rows that were sent. Rows logged while the request was in flight have
/// other keys and stay queued. One transaction, so the counts and the outbox
/// never disagree about whether a row was counted. Mirrors `settleOutbox` in
/// `extension/src/background/idb.ts`.
pub async fn settle_outbox(
    sent: &[ReviewLog],
    counts: &std::collections::HashMap<String, u32>,
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
    store
        .clear()
        .map_err(|e| e.to_string())?
        .await
        .map_err(|e| e.to_string())?;
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

#[allow(dead_code)]
async fn _unused_run_rw() {
    let _ = run_rw::<_, ()>(|_| Ok(())).await;
}
