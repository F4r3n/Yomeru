//! SQLite card store: the native counterpart of `idb.rs`, with the same
//! public API so callers don't care which one they got.
//!
//! One table per IndexedDB object store. Rows hold the serde-JSON of the
//! value (exactly what IndexedDB would store), plus the key columns needed for
//! lookups. Each public function is one SQLite transaction, which gives the
//! same atomicity the IndexedDB version gets from one IDB transaction.
//!
//! The work is synchronous — local SQLite calls on a deck of a few thousand
//! cards take microseconds — and lives in `fn(&Connection)` helpers so tests
//! can run them against an in-memory database. The async wrappers only fetch
//! the shared connection.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use crate::store_common::*;
use crate::sync::SyncCursor;
use crate::types::{CardDirection, CardItem, ReviewLog, SrsCard, card_id};

/// 2: cards keyed on `kind` + `item` instead of a `u32` `sequence`.
const SCHEMA_VERSION: i64 = 2;
const CURSOR_KEY: &str = "sync_cursor";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS cards (
        id   TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        item TEXT NOT NULL,
        json TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS cards_item ON cards(kind, item);
    CREATE TABLE IF NOT EXISTS tombstones (
        id         TEXT PRIMARY KEY,
        deleted_at REAL NOT NULL
    );
    CREATE TABLE IF NOT EXISTS review_outbox (
        card_id     TEXT NOT NULL,
        reviewed_ms REAL NOT NULL,
        json        TEXT NOT NULL,
        PRIMARY KEY (card_id, reviewed_ms)
    );
    CREATE TABLE IF NOT EXISTS drill_counts (
        card_id TEXT PRIMARY KEY,
        count   INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS dirty (
        id TEXT PRIMARY KEY,
        v  REAL NOT NULL
    );
    CREATE TABLE IF NOT EXISTS meta (
        key  TEXT PRIMARY KEY,
        json TEXT NOT NULL
    );
";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Creates the tables on a fresh database, and upgrades an older one the way
/// `idb.rs` upgrades IndexedDB.
///
/// 1 → 2: every card id changed (`"<sequence>::<direction>"` became
/// `"<kind>:<item>:<direction>"`). Nothing is converted: the card data and
/// everything keyed by card id is dropped with the sync cursor, so the next
/// sync is a full one that pulls every card back from the server.
pub(crate) fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(err)?;
    if version < SCHEMA_VERSION {
        if version == 1 {
            conn.execute_batch(
                "DROP TABLE IF EXISTS cards; DROP TABLE IF EXISTS tombstones;
                 DROP TABLE IF EXISTS review_outbox; DROP TABLE IF EXISTS drill_counts;
                 DROP TABLE IF EXISTS dirty; DROP TABLE IF EXISTS meta;",
            )
            .map_err(err)?;
        }
        conn.execute_batch(SCHEMA).map_err(err)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(err)?;
    }
    Ok(())
}

fn conn() -> Result<MutexGuard<'static, Connection>, String> {
    static DB: OnceLock<Result<Mutex<Connection>, String>> = OnceLock::new();
    let db = DB.get_or_init(|| {
        let path = crate::rt::data_dir().join("yomeru.sqlite");
        let conn = Connection::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
        // WAL: a crash mid-write can't corrupt the file, and reads don't block
        // on a write in progress.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(err)?;
        migrate(&conn)?;
        Ok(Mutex::new(conn))
    });
    match db {
        Ok(m) => m.lock().map_err(err),
        Err(e) => Err(e.clone()),
    }
}

/// Runs `f` in one transaction on the shared connection, committing on `Ok`.
fn with_tx<R>(f: impl FnOnce(&Transaction<'_>) -> Result<R, String>) -> Result<R, String> {
    let mut conn = conn()?;
    let tx = conn.transaction().map_err(err)?;
    let out = f(&tx)?;
    tx.commit().map_err(err)?;
    Ok(out)
}

fn to_json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(err)
}

/// Rows that no longer parse are skipped, like `from_value(..).ok()` in
/// `idb.rs`: one bad row must not hide the whole deck.
fn parse<T: DeserializeOwned>(json: &str) -> Option<T> {
    serde_json::from_str(json).ok()
}

fn query_json<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<T>, String> {
    let mut stmt = conn.prepare_cached(sql).map_err(err)?;
    let rows = stmt
        .query_map(args, |r| r.get::<_, String>(0))
        .map_err(err)?;
    let mut out = Vec::new();
    for row in rows {
        if let Some(v) = parse(&row.map_err(err)?) {
            out.push(v);
        }
    }
    Ok(out)
}

// ── Sync helpers on a connection (tested directly) ──────────────────

fn put_card_row(conn: &Connection, card: &SrsCard) -> Result<(), String> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO cards (id, kind, item, json) VALUES (?1, ?2, ?3, ?4)",
    )
    .map_err(err)?
    .execute(params![
        card.id,
        card.item.kind().as_str(),
        card.item.key(),
        to_json(card)?
    ])
    .map_err(err)?;
    Ok(())
}

/// A card as the server sent it, kept whole because this build can't read it
/// (see `idb::put_raw_cards`). Rows without an `id` are dropped.
pub(crate) fn put_raw_card_row(conn: &Connection, card: &serde_json::Value) -> Result<(), String> {
    if !crate::types::is_well_formed_card(card) {
        return Ok(());
    }
    let field = |k: &str| card.get(k).and_then(|v| v.as_str()).unwrap_or_default();
    conn.prepare_cached(
        "INSERT OR REPLACE INTO cards (id, kind, item, json) VALUES (?1, ?2, ?3, ?4)",
    )
    .map_err(err)?
    .execute(params![field("id"), field("kind"), field("item"), to_json(card)?])
    .map_err(err)?;
    Ok(())
}

fn mark_dirty(conn: &Connection, id: &str, v: f64) -> Result<(), String> {
    conn.prepare_cached("INSERT OR REPLACE INTO dirty (id, v) VALUES (?1, ?2)")
        .map_err(err)?
        .execute(params![id, v])
        .map_err(err)?;
    Ok(())
}

pub(crate) fn put_all_on(
    conn: &Connection,
    cards: &[SrsCard],
    stamp: bool,
    now: f64,
) -> Result<(), String> {
    for c in cards {
        if stamp {
            let mut c = c.clone();
            c.updated_ms = now;
            put_card_row(conn, &c)?;
            mark_dirty(conn, &c.id, now)?;
        } else {
            put_card_row(conn, c)?;
        }
    }
    Ok(())
}

pub(crate) fn get_all_cards_on(conn: &Connection) -> Result<Vec<SrsCard>, String> {
    let cards: Vec<SrsCard> = query_json(conn, "SELECT json FROM cards", [])?;
    // Cards stored before `updated_ms` existed deserialize with 0; see the
    // matching comment in `idb.rs`.
    Ok(cards
        .into_iter()
        .map(|mut c| {
            c.updated_ms = c.version_ms();
            c
        })
        .collect())
}

pub(crate) fn delete_with_tombstones_on(
    conn: &Connection,
    ids: &[String],
    now: f64,
) -> Result<(), String> {
    for id in ids {
        conn.prepare_cached("INSERT OR REPLACE INTO tombstones (id, deleted_at) VALUES (?1, ?2)")
            .map_err(err)?
            .execute(params![id, now])
            .map_err(err)?;
        conn.prepare_cached("DELETE FROM cards WHERE id = ?1")
            .map_err(err)?
            .execute([id])
            .map_err(err)?;
    }
    Ok(())
}

pub(crate) fn get_all_tombstones_on(conn: &Connection) -> Result<Vec<Tombstone>, String> {
    let mut stmt = conn
        .prepare_cached("SELECT id, deleted_at FROM tombstones WHERE id <> ''")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Tombstone {
                id: r.get(0)?,
                deleted_at: r.get(1)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn delete_ids(conn: &Connection, table: &str, ids: &[String]) -> Result<(), String> {
    let mut stmt = conn
        .prepare_cached(&format!("DELETE FROM {table} WHERE id = ?1"))
        .map_err(err)?;
    for id in ids {
        stmt.execute([id]).map_err(err)?;
    }
    Ok(())
}

fn add_review_on(conn: &Connection, row: &ReviewLog) -> Result<(), String> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO review_outbox (card_id, reviewed_ms, json) VALUES (?1, ?2, ?3)",
    )
    .map_err(err)?
    .execute(params![row.card_id, row.reviewed_ms, to_json(row)?])
    .map_err(err)?;
    Ok(())
}

fn get_outbox_on(conn: &Connection) -> Result<Vec<ReviewLog>, String> {
    // Key order, like an IDB getAll over the compound key.
    query_json(
        conn,
        "SELECT json FROM review_outbox ORDER BY card_id, reviewed_ms",
        [],
    )
}

pub(crate) fn get_drill_state_on(
    conn: &Connection,
) -> Result<(HashMap<String, u32>, Vec<ReviewLog>), String> {
    let mut stmt = conn
        .prepare_cached("SELECT card_id, count FROM drill_counts")
        .map_err(err)?;
    let counts = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))
        .map_err(err)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(err)?;
    Ok((counts, get_outbox_on(conn)?))
}

pub(crate) fn settle_outbox_on(
    conn: &Connection,
    sent: &[ReviewLog],
    counts: &HashMap<String, u32>,
    replace: bool,
) -> Result<(), String> {
    {
        let mut del = conn
            .prepare_cached("DELETE FROM review_outbox WHERE card_id = ?1 AND reviewed_ms = ?2")
            .map_err(err)?;
        for r in sent {
            del.execute(params![r.card_id, r.reviewed_ms])
                .map_err(err)?;
        }
    }
    if replace {
        conn.execute("DELETE FROM drill_counts", []).map_err(err)?;
    }
    let mut put = conn
        .prepare_cached("INSERT OR REPLACE INTO drill_counts (card_id, count) VALUES (?1, ?2)")
        .map_err(err)?;
    for (card_id, count) in counts {
        put.execute(params![card_id, count]).map_err(err)?;
    }
    Ok(())
}

pub(crate) fn get_dirty_on(conn: &Connection) -> Result<Vec<DirtyEntry>, String> {
    let mut stmt = conn
        .prepare_cached("SELECT id, v FROM dirty")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DirtyEntry {
                id: r.get(0)?,
                v: r.get(1)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub(crate) fn clear_dirty_on(conn: &Connection, snapshot: &[DirtyEntry]) -> Result<(), String> {
    // Only entries not re-stamped since the snapshot: see `idb::clear_dirty`.
    let mut stmt = conn
        .prepare_cached("DELETE FROM dirty WHERE id = ?1 AND v = ?2")
        .map_err(err)?;
    for e in snapshot {
        stmt.execute(params![e.id, e.v]).map_err(err)?;
    }
    Ok(())
}

pub(crate) fn get_cursor_on(conn: &Connection) -> Result<Option<SyncCursor>, String> {
    let json: Option<String> = conn
        .query_row("SELECT json FROM meta WHERE key = ?1", [CURSOR_KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(err)?;
    Ok(json.and_then(|j| parse(&j)))
}

pub(crate) fn set_cursor_on(conn: &Connection, cursor: Option<&SyncCursor>) -> Result<(), String> {
    match cursor {
        Some(c) => conn.execute(
            "INSERT OR REPLACE INTO meta (key, json) VALUES (?1, ?2)",
            params![CURSOR_KEY, to_json(c)?],
        ),
        None => conn.execute("DELETE FROM meta WHERE key = ?1", [CURSOR_KEY]),
    }
    .map_err(err)?;
    Ok(())
}

// ── Public async API (mirrors idb.rs) ───────────────────────────────

/// Writes `cards`, optionally stamping each with a fresh `updated_ms` and
/// queueing it in the dirty set. See `put_all` in `idb.rs` for why every
/// local write funnels through here.
pub(crate) async fn put_all(cards: &[SrsCard], stamp: bool) -> Result<(), String> {
    if cards.is_empty() {
        return Ok(());
    }
    let now = crate::rt::now_ms();
    with_tx(|tx| put_all_on(tx, cards, stamp, now))
}

/// Writes cards this build can't read, unstamped. See `idb::put_raw_cards`.
pub(crate) async fn put_raw_cards(cards: &[serde_json::Value]) -> Result<(), String> {
    if cards.is_empty() {
        return Ok(());
    }
    with_tx(|tx| cards.iter().try_for_each(|c| put_raw_card_row(tx, c)))
}

pub async fn get_card(item: CardItem, direction: CardDirection) -> Result<Option<SrsCard>, String> {
    let conn = conn()?;
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM cards WHERE id = ?1",
            [card_id(item, direction)],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(json.and_then(|j| parse(&j)))
}

/// Every card of `item`. See `idb::get_cards_of`.
pub async fn get_cards_of(item: CardItem) -> Result<Vec<SrsCard>, String> {
    get_cards_of_on(&*conn()?, item)
}

pub(crate) fn get_cards_of_on(conn: &Connection, item: CardItem) -> Result<Vec<SrsCard>, String> {
    query_json(
        conn,
        "SELECT json FROM cards WHERE kind = ?1 AND item = ?2",
        params![item.kind().as_str(), item.key()],
    )
}

pub async fn has_card(item: CardItem) -> Result<bool, String> {
    conn()?
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM cards WHERE kind = ?1 AND item = ?2)",
            params![item.kind().as_str(), item.key()],
            |r| r.get(0),
        )
        .map_err(err)
}

pub async fn get_all_cards() -> Result<Vec<SrsCard>, String> {
    get_all_cards_on(&*conn()?)
}

/// Deletes the given card ids and writes a tombstone for each, atomically, so
/// a crash mid-delete can't lose the tombstone and let a sync resurrect it.
pub(crate) async fn delete_ids_with_tombstones(ids: &[String]) -> Result<(), String> {
    let now = crate::rt::now_ms();
    with_tx(|tx| delete_with_tombstones_on(tx, ids, now))
}

pub async fn get_all_tombstones() -> Result<Vec<Tombstone>, String> {
    get_all_tombstones_on(&*conn()?)
}

pub async fn clear_tombstones(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    with_tx(|tx| delete_ids(tx, "tombstones", ids))
}

/// Applies tombstones that came from the server: deletes the card rows
/// without writing local tombstones. See `idb::apply_remote_deletions`.
pub async fn apply_remote_deletions(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    with_tx(|tx| delete_ids(tx, "cards", ids))
}

/// Saves a reviewed card and queues its answer, in one transaction. Stamps
/// `updated_ms` like [`put_card`].
pub async fn put_card_logged(card: &SrsCard, row: &ReviewLog) -> Result<(), String> {
    let mut card = card.clone();
    card.updated_ms = crate::rt::now_ms();
    with_tx(|tx| {
        put_card_row(tx, &card)?;
        mark_dirty(tx, &card.id, card.updated_ms)?;
        add_review_on(tx, row)
    })
}

/// Undoes an answer in one transaction. See `idb::undo_answer`.
pub async fn undo_answer(
    prev: Option<&SrsCard>,
    undone: &ReviewLog,
    undo: &ReviewLog,
) -> Result<(), String> {
    let prev = prev.map(|c| {
        let mut card = c.clone();
        card.updated_ms = crate::rt::now_ms();
        card
    });
    with_tx(|tx| undo_answer_on(tx, prev.as_ref(), undone, undo))
}

fn undo_answer_on(
    conn: &Connection,
    prev: Option<&SrsCard>,
    undone: &ReviewLog,
    undo: &ReviewLog,
) -> Result<(), String> {
    if let Some(card) = prev {
        put_card_row(conn, card)?;
        mark_dirty(conn, &card.id, card.updated_ms)?;
    }
    conn.prepare_cached("DELETE FROM review_outbox WHERE card_id = ?1 AND reviewed_ms = ?2")
        .map_err(err)?
        .execute(params![undone.card_id, undone.reviewed_ms])
        .map_err(err)?;
    add_review_on(conn, undo)
}

/// Queues one answer for the next sync.
pub async fn add_review(row: &ReviewLog) -> Result<(), String> {
    add_review_on(&*conn()?, row)
}

/// Every answer not yet acknowledged by the server.
pub async fn get_outbox() -> Result<Vec<ReviewLog>, String> {
    get_outbox_on(&*conn()?)
}

/// The server's drill counts from the last sync and the rows still waiting to
/// be uploaded, read together. See `idb::get_drill_state`.
pub async fn get_drill_state() -> Result<(HashMap<String, u32>, Vec<ReviewLog>), String> {
    with_tx(|tx| get_drill_state_on(tx))
}

/// Settles an upload the server acknowledged. See `idb::settle_outbox`.
pub async fn settle_outbox(
    sent: &[ReviewLog],
    counts: &HashMap<String, u32>,
    replace: bool,
) -> Result<(), String> {
    with_tx(|tx| settle_outbox_on(tx, sent, counts, replace))
}

pub async fn get_dirty() -> Result<Vec<DirtyEntry>, String> {
    get_dirty_on(&*conn()?)
}

/// The cards behind a dirty snapshot. Ids deleted since are skipped.
pub async fn get_cards_by_ids(ids: &[String]) -> Result<Vec<SrsCard>, String> {
    let conn = conn()?;
    let mut stmt = conn
        .prepare_cached("SELECT json FROM cards WHERE id = ?1")
        .map_err(err)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let json: Option<String> = stmt.query_row([id], |r| r.get(0)).optional().map_err(err)?;
        if let Some(c) = json.and_then(|j| parse(&j)) {
            out.push(c);
        }
    }
    Ok(out)
}

pub async fn clear_dirty(snapshot: &[DirtyEntry]) -> Result<(), String> {
    if snapshot.is_empty() {
        return Ok(());
    }
    with_tx(|tx| clear_dirty_on(tx, snapshot))
}

/// Where this device's view of the server ends. Kept in the same database as
/// the cards, so wiping one wipes the other (see `idb::get_sync_cursor`).
pub async fn get_sync_cursor() -> Result<Option<SyncCursor>, String> {
    get_cursor_on(&*conn()?)
}

pub async fn set_sync_cursor(cursor: Option<&SyncCursor>) -> Result<(), String> {
    set_cursor_on(&*conn()?, cursor)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::types::{CardStatus, ReviewKind};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn card(sequence: u32, direction: CardDirection) -> SrsCard {
        let mut c = SrsCard::new(CardItem::Word(sequence), direction, 1_000.0);
        c.status = CardStatus::Active;
        c
    }

    fn review(card_id: &str, at: f64, kind: ReviewKind) -> ReviewLog {
        ReviewLog {
            card_id: card_id.into(),
            rating: 3,
            kind,
            reviewed_ms: at,
            undoes_ms: None,
        }
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = db();
        migrate(&conn).unwrap();
    }

    #[test]
    fn version_1_card_data_is_dropped() {
        // A version-1 database: cards keyed on `sequence`, a review waiting
        // and a cursor. All of it is keyed by old card ids, so it goes; the
        // next sync is a full one and pulls the cards back.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cards (id TEXT PRIMARY KEY, sequence INTEGER NOT NULL, json TEXT NOT NULL);
             CREATE TABLE tombstones (id TEXT PRIMARY KEY, deleted_at REAL NOT NULL);
             CREATE TABLE review_outbox (card_id TEXT NOT NULL, reviewed_ms REAL NOT NULL,
                                         json TEXT NOT NULL, PRIMARY KEY (card_id, reviewed_ms));
             CREATE TABLE drill_counts (card_id TEXT PRIMARY KEY, count INTEGER NOT NULL);
             CREATE TABLE dirty (id TEXT PRIMARY KEY, v REAL NOT NULL);
             CREATE TABLE meta (key TEXT PRIMARY KEY, json TEXT NOT NULL);
             INSERT INTO cards VALUES ('1358280::recall', 1358280, '{}');
             INSERT INTO review_outbox VALUES ('1358280::recall', 5.0, '{}');
             INSERT INTO meta VALUES ('sync_cursor', '{\"epoch\":\"e\",\"rev\":3}');
             PRAGMA user_version = 1;",
        )
        .unwrap();
        migrate(&conn).unwrap();

        let count = |t: &str| -> i64 {
            conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap()
        };
        for t in ["cards", "tombstones", "review_outbox", "drill_counts", "dirty", "meta"] {
            assert_eq!(count(t), 0, "{t}");
        }
        assert_eq!(get_cursor_on(&conn).unwrap(), None);
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // The new table takes new cards.
        let a = card(1_358_280, CardDirection::Recall);
        put_all_on(&conn, std::slice::from_ref(&a), false, 0.0).unwrap();
        assert_eq!(get_cards_of_on(&conn, CardItem::Word(1_358_280)).unwrap().len(), 1);
    }

    #[test]
    fn cards_of_an_item_are_its_directions_only() {
        let conn = db();
        let rec = card(1_000_001, CardDirection::Recognition);
        let rec_other = card(1_000_002, CardDirection::Recognition);
        let kanji = SrsCard::new_kanji('猫', 0.0);
        let grammar = SrsCard::new_grammar(1_000_001, 0.0);
        put_all_on(&conn, &[rec.clone(), rec_other, kanji.clone(), grammar.clone()], false, 0.0)
            .unwrap();
        let ids = |item| -> Vec<String> {
            get_cards_of_on(&conn, item).unwrap().into_iter().map(|c| c.id).collect()
        };
        assert_eq!(ids(CardItem::Word(1_000_001)), vec![rec.id]);
        // A grammar entry numbered like a word's ent_seq is another item.
        assert_eq!(ids(CardItem::Grammar(1_000_001)), vec![grammar.id]);
        assert_eq!(ids(CardItem::Kanji('猫')), vec![kanji.id]);
    }

    #[test]
    fn a_card_this_build_cannot_read_is_kept_but_never_read() {
        let conn = db();
        let known = card(1_000_001, CardDirection::Recognition);
        put_all_on(&conn, std::slice::from_ref(&known), false, 0.0).unwrap();
        let mut odd = serde_json::to_value(SrsCard::new_grammar(7, 0.0)).unwrap();
        odd["id"] = "grammar:7:production".into();
        odd["direction"] = "production".into();
        put_raw_card_row(&conn, &odd).unwrap();
        put_raw_card_row(&conn, &serde_json::json!({ "kind": "x" })).unwrap();

        let rows: i64 = conn.query_row("SELECT count(*) FROM cards", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 2, "the odd card is stored, the id-less one dropped");
        let all = get_all_cards_on(&conn).unwrap();
        assert_eq!(all.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [known.id.as_str()]);
        assert!(get_cards_of_on(&conn, CardItem::Grammar(7)).unwrap().is_empty());
        // Kept as it came, for the build that knows its direction.
        let json: String = conn
            .query_row("SELECT json FROM cards WHERE id = 'grammar:7:production'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap(), odd);
    }

    #[test]
    fn stamped_put_marks_dirty_and_synced_put_does_not() {
        let conn = db();
        let a = card(1_000_001, CardDirection::Recognition);
        let b = card(1_000_002, CardDirection::Recognition);
        put_all_on(&conn, std::slice::from_ref(&a), true, 5_000.0).unwrap();
        put_all_on(&conn, std::slice::from_ref(&b), false, 6_000.0).unwrap();

        let dirty = get_dirty_on(&conn).unwrap();
        assert_eq!(
            dirty,
            vec![DirtyEntry {
                id: a.id.clone(),
                v: 5_000.0
            }]
        );
        let all = get_all_cards_on(&conn).unwrap();
        let stored_a = all.iter().find(|c| c.id == a.id).unwrap();
        assert_eq!(stored_a.updated_ms, 5_000.0);
        let stored_b = all.iter().find(|c| c.id == b.id).unwrap();
        assert_eq!(stored_b.updated_ms, b.version_ms());
    }

    #[test]
    fn clear_dirty_keeps_entries_restamped_since_snapshot() {
        let conn = db();
        let a = card(1_000_001, CardDirection::Recognition);
        let b = card(1_000_002, CardDirection::Recall);
        put_all_on(&conn, &[a.clone(), b.clone()], true, 1.0).unwrap();
        let snapshot = get_dirty_on(&conn).unwrap();
        // `a` changes again while the sync is in flight.
        put_all_on(&conn, std::slice::from_ref(&a), true, 2.0).unwrap();
        clear_dirty_on(&conn, &snapshot).unwrap();
        let left = get_dirty_on(&conn).unwrap();
        assert_eq!(left, vec![DirtyEntry { id: a.id, v: 2.0 }]);
    }

    #[test]
    fn delete_writes_tombstones_and_removes_cards() {
        let conn = db();
        let a = card(1_000_001, CardDirection::Recognition);
        put_all_on(&conn, std::slice::from_ref(&a), false, 0.0).unwrap();
        delete_with_tombstones_on(&conn, std::slice::from_ref(&a.id), 42.0).unwrap();
        assert!(get_all_cards_on(&conn).unwrap().is_empty());
        assert_eq!(
            get_all_tombstones_on(&conn).unwrap(),
            vec![Tombstone {
                id: a.id.clone(),
                deleted_at: 42.0
            }]
        );
        delete_ids(&conn, "tombstones", &[a.id]).unwrap();
        assert!(get_all_tombstones_on(&conn).unwrap().is_empty());
    }

    #[test]
    fn settle_outbox_drops_sent_rows_and_merges_or_replaces_counts() {
        let conn = db();
        let sent = review("1::recognition", 10.0, ReviewKind::Drill);
        let later = review("1::recognition", 20.0, ReviewKind::Drill);
        add_review_on(&conn, &sent).unwrap();
        add_review_on(&conn, &later).unwrap();

        let counts = HashMap::from([("1::recognition".to_string(), 3), ("2::recall".into(), 1)]);
        settle_outbox_on(&conn, std::slice::from_ref(&sent), &counts, false).unwrap();
        let (got, outbox) = get_drill_state_on(&conn).unwrap();
        assert_eq!(got, counts);
        assert_eq!(outbox, vec![later]);

        // A delta merges over the cache…
        let delta = HashMap::from([("2::recall".to_string(), 2)]);
        settle_outbox_on(&conn, &[], &delta, false).unwrap();
        let (got, _) = get_drill_state_on(&conn).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got["2::recall"], 2);

        // …a full answer replaces it.
        settle_outbox_on(&conn, &[], &delta, true).unwrap();
        let (got, _) = get_drill_state_on(&conn).unwrap();
        assert_eq!(got, delta);
    }

    #[test]
    fn cursor_round_trips_and_clears() {
        let conn = db();
        assert_eq!(get_cursor_on(&conn).unwrap(), None);
        let c = SyncCursor {
            epoch: "e1".into(),
            rev: 7,
        };
        set_cursor_on(&conn, Some(&c)).unwrap();
        assert_eq!(get_cursor_on(&conn).unwrap(), Some(c));
        set_cursor_on(&conn, None).unwrap();
        assert_eq!(get_cursor_on(&conn).unwrap(), None);
    }
    #[test]
    fn undo_restores_card_and_swaps_answer_for_retraction() {
        let conn = db();
        let before = card(1_000_001, CardDirection::Recall);
        let mut after = before.clone();
        after.reps = 1;
        let answer = review(&before.id, 10.0, ReviewKind::Review);
        add_review_on(&conn, &answer).unwrap();
        put_all_on(&conn, std::slice::from_ref(&after), true, 5_000.0).unwrap();

        let mut prev = before.clone();
        prev.updated_ms = 6_000.0;
        let undo = ReviewLog {
            kind: ReviewKind::Undo,
            reviewed_ms: 11.0,
            undoes_ms: Some(10.0),
            ..answer.clone()
        };
        undo_answer_on(&conn, Some(&prev), &answer, &undo).unwrap();

        let cards = get_all_cards_on(&conn).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].reps, 0);
        assert_eq!(get_outbox_on(&conn).unwrap(), vec![undo]);
        assert!(
            get_dirty_on(&conn)
                .unwrap()
                .iter()
                .any(|d| d.id == prev.id && d.v == 6_000.0)
        );
    }
}
