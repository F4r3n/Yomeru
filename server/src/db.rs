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
/// is newer (higher last_review_ms; NULL treated as 0).
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
