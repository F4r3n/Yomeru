use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

pub type Db = Arc<Mutex<Connection>>;

// 1-hour cooldown between OTP requests for the same email.
const OTP_COOLDOWN_MS: i64 = 3_600_000;
// 10-minute TTL for a generated OTP.
const OTP_TTL_MS: i64 = 600_000;

pub fn init_db(path: &str) -> Db {
    let conn = Connection::open(path).expect("open sqlite db");
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS cards (
             id              TEXT PRIMARY KEY,
             data            TEXT NOT NULL,
             last_review_ms  INTEGER
         );
         CREATE TABLE IF NOT EXISTS otps (
             email           TEXT PRIMARY KEY,
             code            TEXT NOT NULL,
             expires_at      INTEGER NOT NULL,
             last_requested  INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS sessions (
             token       TEXT PRIMARY KEY,
             email       TEXT NOT NULL,
             expires_at  INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS deletions (
             id          TEXT PRIMARY KEY,
             deleted_at  INTEGER NOT NULL
         );",
    )
    .expect("init db schema");
    Arc::new(Mutex::new(conn))
}

/// Returns Err if the per-email 1-hour cooldown is still active.
pub fn store_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> Result<(), ()> {
    let conn = db.lock().unwrap();
    let last: Option<i64> = conn
        .query_row(
            "SELECT last_requested FROM otps WHERE email = ?1",
            params![email],
            |r| r.get(0),
        )
        .ok();
    if let Some(t) = last {
        if now_ms - t < OTP_COOLDOWN_MS {
            return Err(());
        }
    }
    conn.execute(
        "INSERT OR REPLACE INTO otps (email, code, expires_at, last_requested)
         VALUES (?1, ?2, ?3, ?4)",
        params![email, code, now_ms + OTP_TTL_MS, now_ms],
    )
    .unwrap();
    Ok(())
}

/// Validates code and expiry, deletes the OTP row on success.
pub fn verify_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> bool {
    let conn = db.lock().unwrap();
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT code, expires_at FROM otps WHERE email = ?1",
            params![email],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    match row {
        Some((stored, exp)) if stored == code && now_ms < exp => {
            let _ = conn.execute("DELETE FROM otps WHERE email = ?1", params![email]);
            true
        }
        _ => false,
    }
}

pub fn create_session(db: &Db, token: &str, email: &str, expires_at: i64) {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO sessions (token, email, expires_at) VALUES (?1, ?2, ?3)",
        params![token, email, expires_at],
    )
    .unwrap();
}

/// Returns the email associated with a valid (non-expired) session token.
pub fn validate_session(db: &Db, token: &str, now_ms: i64) -> Option<String> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT email FROM sessions WHERE token = ?1 AND expires_at > ?2",
        params![token, now_ms],
        |r| r.get(0),
    )
    .ok()
}

/// Upserts incoming cards: replaces a stored card only if the incoming one
/// is newer (higher last_review_ms; NULL treated as 0). Any tombstone for the
/// same id is cleared — a re-add wins over an old delete.
pub fn upsert_cards(db: &Db, cards: &[serde_json::Value]) {
    let mut conn = db.lock().unwrap();
    let tx = conn.transaction().unwrap();
    for card in cards {
        let id = match card.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let last_review_ms = card.get("last_review_ms").and_then(|v| v.as_i64());
        let data = serde_json::to_string(card).unwrap_or_default();
        tx.execute(
            "INSERT INTO cards (id, data, last_review_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                 data = excluded.data,
                 last_review_ms = excluded.last_review_ms
             WHERE COALESCE(excluded.last_review_ms, 0) >= COALESCE(cards.last_review_ms, 0)",
            params![id, data, last_review_ms],
        )
        .unwrap();
        tx.execute("DELETE FROM deletions WHERE id = ?1", params![id])
            .unwrap();
    }
    tx.commit().unwrap();
}

pub fn get_all_cards(db: &Db) -> Vec<serde_json::Value> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT data FROM cards").unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

/// Applies incoming tombstones: drops each id from `cards` and records the
/// tombstone so other clients can replay the delete.
pub fn apply_deletions(db: &Db, ids: &[String], now_ms: i64) {
    if ids.is_empty() {
        return;
    }
    let mut conn = db.lock().unwrap();
    let tx = conn.transaction().unwrap();
    for id in ids {
        if id.is_empty() {
            continue;
        }
        tx.execute("DELETE FROM cards WHERE id = ?1", params![id])
            .unwrap();
        tx.execute(
            "INSERT INTO deletions (id, deleted_at) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET deleted_at = excluded.deleted_at",
            params![id, now_ms],
        )
        .unwrap();
    }
    tx.commit().unwrap();
}

pub fn get_all_deletions(db: &Db) -> Vec<String> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id FROM deletions").unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

/// Prunes tombstones older than `cutoff_ms`. Called at startup to keep the
/// table bounded; 90 days is well over any reasonable offline window.
pub fn prune_old_deletions(db: &Db, cutoff_ms: i64) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "DELETE FROM deletions WHERE deleted_at < ?1",
        params![cutoff_ms],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fresh_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cards (
                 id              TEXT PRIMARY KEY,
                 data            TEXT NOT NULL,
                 last_review_ms  INTEGER
             );
             CREATE TABLE deletions (
                 id          TEXT PRIMARY KEY,
                 deleted_at  INTEGER NOT NULL
             );",
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn card(id: &str, last_review_ms: Option<i64>) -> serde_json::Value {
        json!({ "id": id, "last_review_ms": last_review_ms, "word": "猫" })
    }

    #[test]
    fn upsert_inserts_new_cards() {
        let db = fresh_db();
        upsert_cards(&db, &[card("a::recognition", Some(100))]);
        let stored = get_all_cards(&db);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["id"], "a::recognition");
    }

    #[test]
    fn upsert_keeps_newer_last_review() {
        let db = fresh_db();
        upsert_cards(&db, &[card("a::recognition", Some(100))]);
        // Older incoming write should be ignored.
        upsert_cards(&db, &[card("a::recognition", Some(50))]);
        let stored = get_all_cards(&db);
        assert_eq!(stored[0]["last_review_ms"], 100);
    }

    #[test]
    fn upsert_replaces_with_equal_or_newer_last_review() {
        let db = fresh_db();
        upsert_cards(&db, &[card("a::recognition", Some(100))]);
        let mut newer = card("a::recognition", Some(200));
        newer["word"] = json!("犬");
        upsert_cards(&db, &[newer]);
        let stored = get_all_cards(&db);
        assert_eq!(stored[0]["word"], "犬");
        assert_eq!(stored[0]["last_review_ms"], 200);
    }

    #[test]
    fn apply_deletions_removes_card_and_records_tombstone() {
        let db = fresh_db();
        upsert_cards(&db, &[card("a::recognition", Some(100))]);
        apply_deletions(&db, &["a::recognition".to_string()], 1_700_000_000_000);
        assert!(get_all_cards(&db).is_empty());
        assert_eq!(get_all_deletions(&db), vec!["a::recognition".to_string()]);
    }

    #[test]
    fn upsert_clears_matching_tombstone() {
        // Re-add must win over an old delete: otherwise a client that brings
        // back a card after deleting it would see the resurrection wiped
        // out on the next sync.
        let db = fresh_db();
        apply_deletions(&db, &["a::recognition".to_string()], 1_000);
        upsert_cards(&db, &[card("a::recognition", Some(2_000))]);
        assert_eq!(get_all_cards(&db).len(), 1);
        assert!(get_all_deletions(&db).is_empty());
    }

    #[test]
    fn apply_deletions_is_idempotent() {
        let db = fresh_db();
        apply_deletions(&db, &["a::recognition".to_string()], 100);
        apply_deletions(&db, &["a::recognition".to_string()], 200);
        let tombs = get_all_deletions(&db);
        assert_eq!(tombs.len(), 1);
    }

    #[test]
    fn apply_deletions_skips_empty_ids() {
        let db = fresh_db();
        apply_deletions(&db, &[String::new(), "a".into()], 100);
        assert_eq!(get_all_deletions(&db), vec!["a".to_string()]);
    }

    #[test]
    fn prune_drops_only_old_tombstones() {
        let db = fresh_db();
        apply_deletions(&db, &["old".into()], 100);
        apply_deletions(&db, &["recent".into()], 5_000);
        prune_old_deletions(&db, 1_000);
        assert_eq!(get_all_deletions(&db), vec!["recent".to_string()]);
    }
}
