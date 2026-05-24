use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use std::time::Duration;

pub type Db = SqlitePool;

/// A spaced-repetition card as exchanged with clients and stored one field per
/// column. The server owns this shape now: adding a field on the client means
/// adding a column here (and a migration) or the field is dropped on round-trip.
/// Enum-typed client fields (`direction`, `state`, `status`) are kept as their
/// lowercase string form — the server is a relay and doesn't interpret them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub word: String,
    pub direction: String,
    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: i64,
    pub lapses: i64,
    pub state: String,
    #[serde(default)]
    pub last_review_ms: Option<f64>,
    pub added_ms: f64,
    pub status: String,
}

/// A user's synced scheduler settings. One row per email. `updated_ms` is the
/// last-write-wins merge key (wall-clock ms of the client edit that produced
/// these values). Device-local fields (server URL/email/token) are never
/// stored here — only the knobs that affect scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub graduation_reps: i64,
    pub interval_scale: f64,
    pub max_session_cards: i64,
    /// FSRS desired retention. Defaulted so a client that predates this field
    /// can still sync its other settings without failing the whole request.
    #[serde(default = "default_request_retention")]
    pub request_retention: f64,
    pub updated_ms: f64,
}

fn default_request_retention() -> f64 {
    0.9
}

const SETTINGS_DDL: &str = "CREATE TABLE IF NOT EXISTS settings (
             email             TEXT PRIMARY KEY,
             graduation_reps   INTEGER NOT NULL,
             interval_scale    REAL NOT NULL,
             max_session_cards INTEGER NOT NULL,
             request_retention REAL NOT NULL,
             updated_ms        REAL NOT NULL
         )";

/// New per-column `cards` schema. `last_review_ms` is the sync merge key and is
/// nullable (never-reviewed cards have no value); everything else is required.
const CARDS_DDL: &str = "CREATE TABLE IF NOT EXISTS cards (
             email           TEXT NOT NULL,
             id              TEXT NOT NULL,
             word            TEXT NOT NULL,
             direction       TEXT NOT NULL,
             due_ms          REAL NOT NULL,
             stability       REAL NOT NULL,
             difficulty      REAL NOT NULL,
             reps            INTEGER NOT NULL,
             lapses          INTEGER NOT NULL,
             state           TEXT NOT NULL,
             last_review_ms  REAL,
             added_ms        REAL NOT NULL,
             status          TEXT NOT NULL,
             PRIMARY KEY (email, id)
         )";

// Minimum gap between OTP emails for the same address — anti-spam only. A new
// code is always allowed once the previous one has expired, so a user who
// misses the TTL is never stranded waiting out a long cooldown.
const OTP_RESEND_FLOOR_MS: i64 = 60_000;
// 10-minute TTL for a generated OTP.
const OTP_TTL_MS: i64 = 600_000;

pub async fn init_db(path: &str) -> anyhow::Result<Db> {
    let opts = SqliteConnectOptions::from_str(path)
        .with_context(|| format!("parse sqlite path {path}"))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .connect_with(opts)
        .await
        .with_context(|| format!("open sqlite db at {path}"))?;
    init_schema(&pool).await?;
    Ok(pool)
}

async fn init_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    migrate_cards_from_blob(pool).await?;
    let stmts = [
        CARDS_DDL,
        SETTINGS_DDL,
        "CREATE TABLE IF NOT EXISTS otps (
             email           TEXT PRIMARY KEY,
             code            TEXT NOT NULL,
             expires_at      INTEGER NOT NULL,
             last_requested  INTEGER NOT NULL
         )",
        "CREATE TABLE IF NOT EXISTS sessions (
             token       TEXT PRIMARY KEY,
             email       TEXT NOT NULL,
             expires_at  INTEGER NOT NULL
         )",
        "CREATE TABLE IF NOT EXISTS deletions (
             email       TEXT NOT NULL,
             id          TEXT NOT NULL,
             deleted_at  INTEGER NOT NULL,
             PRIMARY KEY (email, id)
         )",
    ];
    for s in stmts {
        sqlx::query(s)
            .execute(pool)
            .await
            .context("init db schema")?;
    }
    Ok(())
}

/// One-time migration from the legacy single-`data`-blob `cards` table to the
/// per-column layout. Detected by the presence of a `data` column. Every field
/// is read out of the JSON blob — crucially `last_review_ms`, which the old
/// promoted column failed to populate (it parsed the float merge key with
/// `as_i64`, always yielding NULL). No-op on a fresh or already-migrated DB.
async fn migrate_cards_from_blob(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
        .fetch_all(pool)
        .await
        .context("inspect cards columns")?;
    if !cols.iter().any(|c| c == "data") {
        return Ok(()); // fresh DB or already on the column layout
    }

    let mut tx = pool.begin().await.context("begin cards migration tx")?;
    let stmts = [
        "ALTER TABLE cards RENAME TO cards_blob_old",
        CARDS_DDL,
        // COALESCE guards a stray legacy row missing a field from tripping the
        // NOT NULL columns; last_review_ms stays nullable so it isn't defaulted.
        "INSERT INTO cards
             (email, id, word, direction, due_ms, stability, difficulty,
              reps, lapses, state, last_review_ms, added_ms, status)
         SELECT email,
                id,
                COALESCE(json_extract(data, '$.word'), ''),
                COALESCE(json_extract(data, '$.direction'), 'recognition'),
                COALESCE(CAST(json_extract(data, '$.due_ms') AS REAL), 0),
                COALESCE(CAST(json_extract(data, '$.stability') AS REAL), 0),
                COALESCE(CAST(json_extract(data, '$.difficulty') AS REAL), 0),
                COALESCE(CAST(json_extract(data, '$.reps') AS INTEGER), 0),
                COALESCE(CAST(json_extract(data, '$.lapses') AS INTEGER), 0),
                COALESCE(json_extract(data, '$.state'), 'new'),
                CAST(json_extract(data, '$.last_review_ms') AS REAL),
                COALESCE(CAST(json_extract(data, '$.added_ms') AS REAL), 0),
                COALESCE(json_extract(data, '$.status'), 'active')
         FROM cards_blob_old",
        "DROP TABLE cards_blob_old",
    ];
    for s in stmts {
        sqlx::query(s)
            .execute(&mut *tx)
            .await
            .context("migrate cards from blob")?;
    }
    tx.commit().await.context("commit cards migration tx")?;
    Ok(())
}

/// `Ok(true)` = stored, `Ok(false)` = blocked by the anti-spam floor while a
/// still-valid code exists, `Err` = db error.
///
/// SELECT + INSERT are wrapped in a transaction so concurrent OTP requests
/// for the same email can't both pass the resend check.
pub async fn store_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> anyhow::Result<bool> {
    let mut tx = db.begin().await.context("begin store_otp tx")?;
    let prev: Option<(i64, i64)> =
        sqlx::query_as("SELECT last_requested, expires_at FROM otps WHERE email = ?1")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await
            .context("read otp resend state")?;
    if let Some((last, exp)) = prev {
        // Only throttle while the existing code is still usable. An expired
        // code never blocks a resend, so the TTL can't strand the user.
        if now_ms < exp && now_ms - last < OTP_RESEND_FLOOR_MS {
            return Ok(false);
        }
    }
    sqlx::query(
        "INSERT OR REPLACE INTO otps (email, code, expires_at, last_requested)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(email)
    .bind(code)
    .bind(now_ms + OTP_TTL_MS)
    .bind(now_ms)
    .execute(&mut *tx)
    .await
    .context("insert otp")?;
    tx.commit().await.context("commit store_otp tx")?;
    Ok(true)
}

/// Validates code and expiry, deletes the OTP row on success.
pub async fn verify_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> anyhow::Result<bool> {
    let mut tx = db.begin().await.context("begin verify_otp tx")?;
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT code, expires_at FROM otps WHERE email = ?1")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await
            .context("read otp")?;
    let ok = match row {
        Some((stored, exp)) if stored == code && now_ms < exp => {
            sqlx::query("DELETE FROM otps WHERE email = ?1")
                .bind(email)
                .execute(&mut *tx)
                .await
                .context("delete otp on verify")?;
            true
        }
        _ => false,
    };
    tx.commit().await.context("commit verify_otp tx")?;
    Ok(ok)
}

pub async fn create_session(
    db: &Db,
    token: &str,
    email: &str,
    expires_at: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO sessions (token, email, expires_at) VALUES (?1, ?2, ?3)")
        .bind(token)
        .bind(email)
        .bind(expires_at)
        .execute(db)
        .await
        .context("insert session")?;
    Ok(())
}

/// Returns the email associated with a valid (non-expired) session token, or
/// `Ok(None)` if there is no matching live session.
pub async fn validate_session(db: &Db, token: &str, now_ms: i64) -> anyhow::Result<Option<String>> {
    let email: Option<String> =
        sqlx::query_scalar("SELECT email FROM sessions WHERE token = ?1 AND expires_at > ?2")
            .bind(token)
            .bind(now_ms)
            .fetch_optional(db)
            .await
            .context("validate session")?;
    Ok(email)
}

/// Upserts incoming cards for `email`: replaces a stored card only if the
/// incoming one is newer (higher last_review_ms; NULL treated as 0). Any
/// tombstone for the same (email, id) is cleared — a re-add wins over an
/// old delete.
pub async fn upsert_cards(db: &Db, email: &str, cards: &[Card]) -> anyhow::Result<()> {
    let mut tx = db.begin().await.context("begin upsert tx")?;
    for c in cards {
        if c.id.is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT INTO cards
                 (email, id, word, direction, due_ms, stability, difficulty,
                  reps, lapses, state, last_review_ms, added_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(email, id) DO UPDATE SET
                 word = excluded.word,
                 direction = excluded.direction,
                 due_ms = excluded.due_ms,
                 stability = excluded.stability,
                 difficulty = excluded.difficulty,
                 reps = excluded.reps,
                 lapses = excluded.lapses,
                 state = excluded.state,
                 last_review_ms = excluded.last_review_ms,
                 added_ms = excluded.added_ms,
                 status = excluded.status
             WHERE COALESCE(excluded.last_review_ms, 0) >= COALESCE(cards.last_review_ms, 0)",
        )
        .bind(email)
        .bind(&c.id)
        .bind(&c.word)
        .bind(&c.direction)
        .bind(c.due_ms)
        .bind(c.stability)
        .bind(c.difficulty)
        .bind(c.reps)
        .bind(c.lapses)
        .bind(&c.state)
        .bind(c.last_review_ms)
        .bind(c.added_ms)
        .bind(&c.status)
        .execute(&mut *tx)
        .await
        .context("upsert card")?;
        sqlx::query("DELETE FROM deletions WHERE email = ?1 AND id = ?2")
            .bind(email)
            .bind(&c.id)
            .execute(&mut *tx)
            .await
            .context("clear matching tombstone")?;
    }
    tx.commit().await.context("commit upsert tx")?;
    Ok(())
}

pub async fn get_all_cards(db: &Db, email: &str) -> anyhow::Result<Vec<Card>> {
    let rows = sqlx::query(
        "SELECT id, word, direction, due_ms, stability, difficulty,
                reps, lapses, state, last_review_ms, added_ms, status
         FROM cards WHERE email = ?1",
    )
    .bind(email)
    .fetch_all(db)
    .await
    .context("query get_all_cards")?;
    let cards = rows
        .iter()
        .map(|r| Card {
            id: r.get("id"),
            word: r.get("word"),
            direction: r.get("direction"),
            due_ms: r.get("due_ms"),
            stability: r.get("stability"),
            difficulty: r.get("difficulty"),
            reps: r.get("reps"),
            lapses: r.get("lapses"),
            state: r.get("state"),
            last_review_ms: r.get("last_review_ms"),
            added_ms: r.get("added_ms"),
            status: r.get("status"),
        })
        .collect();
    Ok(cards)
}

/// Applies incoming tombstones for `email`: drops each id from `cards` and
/// records the tombstone so the user's other clients can replay the delete.
pub async fn apply_deletions(
    db: &Db,
    email: &str,
    ids: &[String],
    now_ms: i64,
) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = db.begin().await.context("begin deletions tx")?;
    for id in ids {
        if id.is_empty() {
            continue;
        }
        sqlx::query("DELETE FROM cards WHERE email = ?1 AND id = ?2")
            .bind(email)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("delete card")?;
        sqlx::query(
            "INSERT INTO deletions (email, id, deleted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(email, id) DO UPDATE SET deleted_at = excluded.deleted_at",
        )
        .bind(email)
        .bind(id)
        .bind(now_ms)
        .execute(&mut *tx)
        .await
        .context("upsert tombstone")?;
    }
    tx.commit().await.context("commit deletions tx")?;
    Ok(())
}

pub async fn get_all_deletions(db: &Db, email: &str) -> anyhow::Result<Vec<String>> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM deletions WHERE email = ?1")
        .bind(email)
        .fetch_all(db)
        .await
        .context("query get_all_deletions")?;
    Ok(ids)
}

/// Upserts a user's scheduler settings, last-write-wins: the incoming row
/// replaces the stored one only if its `updated_ms` is greater than or equal
/// to what's stored (ties favor the incoming write, matching the cards merge).
pub async fn upsert_settings(db: &Db, email: &str, s: &Settings) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings
             (email, graduation_reps, interval_scale, max_session_cards,
              request_retention, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(email) DO UPDATE SET
             graduation_reps = excluded.graduation_reps,
             interval_scale = excluded.interval_scale,
             max_session_cards = excluded.max_session_cards,
             request_retention = excluded.request_retention,
             updated_ms = excluded.updated_ms
         WHERE excluded.updated_ms >= settings.updated_ms",
    )
    .bind(email)
    .bind(s.graduation_reps)
    .bind(s.interval_scale)
    .bind(s.max_session_cards)
    .bind(s.request_retention)
    .bind(s.updated_ms)
    .execute(db)
    .await
    .context("upsert settings")?;
    Ok(())
}

/// Returns the user's stored settings, or `Ok(None)` if they've never synced
/// any (so the client keeps its local defaults).
pub async fn get_settings(db: &Db, email: &str) -> anyhow::Result<Option<Settings>> {
    let row = sqlx::query(
        "SELECT graduation_reps, interval_scale, max_session_cards,
                request_retention, updated_ms
         FROM settings WHERE email = ?1",
    )
    .bind(email)
    .fetch_optional(db)
    .await
    .context("query get_settings")?;
    Ok(row.map(|r| Settings {
        graduation_reps: r.get("graduation_reps"),
        interval_scale: r.get("interval_scale"),
        max_session_cards: r.get("max_session_cards"),
        request_retention: r.get("request_retention"),
        updated_ms: r.get("updated_ms"),
    }))
}

/// Prunes tombstones older than `cutoff_ms`. Called at startup to keep the
/// table bounded; 90 days is well over any reasonable offline window.
pub async fn prune_old_deletions(db: &Db, cutoff_ms: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM deletions WHERE deleted_at < ?1")
        .bind(cutoff_ms)
        .execute(db)
        .await
        .context("prune deletions")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // `:memory:` databases live per-connection in SQLite, so a multi-connection
    // pool would see a fresh empty DB on each checkout. Pin to one connection.
    async fn fresh_db() -> Db {
        single_conn_mem().await
    }

    async fn single_conn_mem() -> Db {
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        init_schema(&pool).await.unwrap();
        pool
    }

    const ALICE: &str = "alice@example.com";
    const BOB: &str = "bob@example.com";

    fn card(id: &str, last_review_ms: Option<f64>) -> Card {
        Card {
            id: id.to_string(),
            word: "猫".to_string(),
            direction: "recognition".to_string(),
            due_ms: 0.0,
            stability: 0.0,
            difficulty: 0.0,
            reps: 0,
            lapses: 0,
            state: "new".to_string(),
            last_review_ms,
            added_ms: 0.0,
            status: "active".to_string(),
        }
    }

    #[tokio::test]
    async fn upsert_inserts_new_cards() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))])
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "a::recognition");
    }

    #[tokio::test]
    async fn upsert_keeps_newer_last_review() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))])
            .await
            .unwrap();
        // Older incoming write should be ignored.
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(50.0))])
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].last_review_ms, Some(100.0));
    }

    #[tokio::test]
    async fn upsert_replaces_with_equal_or_newer_last_review() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))])
            .await
            .unwrap();
        let mut newer = card("a::recognition", Some(200.0));
        newer.word = "犬".to_string();
        upsert_cards(&db, ALICE, &[newer]).await.unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].word, "犬");
        assert_eq!(stored[0].last_review_ms, Some(200.0));
    }

    #[tokio::test]
    async fn apply_deletions_removes_card_and_records_tombstone() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))])
            .await
            .unwrap();
        apply_deletions(
            &db,
            ALICE,
            &["a::recognition".to_string()],
            1_700_000_000_000,
        )
        .await
        .unwrap();
        assert!(get_all_cards(&db, ALICE).await.unwrap().is_empty());
        assert_eq!(
            get_all_deletions(&db, ALICE).await.unwrap(),
            vec!["a::recognition".to_string()]
        );
    }

    #[tokio::test]
    async fn upsert_clears_matching_tombstone() {
        // Re-add must win over an old delete: otherwise a client that brings
        // back a card after deleting it would see the resurrection wiped
        // out on the next sync.
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &["a::recognition".to_string()], 1_000)
            .await
            .unwrap();
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(2_000.0))])
            .await
            .unwrap();
        assert_eq!(get_all_cards(&db, ALICE).await.unwrap().len(), 1);
        assert!(get_all_deletions(&db, ALICE).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn apply_deletions_is_idempotent() {
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &["a::recognition".to_string()], 100)
            .await
            .unwrap();
        apply_deletions(&db, ALICE, &["a::recognition".to_string()], 200)
            .await
            .unwrap();
        let tombs = get_all_deletions(&db, ALICE).await.unwrap();
        assert_eq!(tombs.len(), 1);
    }

    #[tokio::test]
    async fn apply_deletions_skips_empty_ids() {
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &[String::new(), "a".into()], 100)
            .await
            .unwrap();
        assert_eq!(
            get_all_deletions(&db, ALICE).await.unwrap(),
            vec!["a".to_string()]
        );
    }

    #[tokio::test]
    async fn prune_drops_only_old_tombstones() {
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &["old".into()], 100)
            .await
            .unwrap();
        apply_deletions(&db, ALICE, &["recent".into()], 5_000)
            .await
            .unwrap();
        prune_old_deletions(&db, 1_000).await.unwrap();
        assert_eq!(
            get_all_deletions(&db, ALICE).await.unwrap(),
            vec!["recent".to_string()]
        );
    }

    #[tokio::test]
    async fn users_cannot_see_each_others_cards() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))])
            .await
            .unwrap();
        upsert_cards(&db, BOB, &[card("b::recognition", Some(200.0))])
            .await
            .unwrap();
        let alice_cards = get_all_cards(&db, ALICE).await.unwrap();
        let bob_cards = get_all_cards(&db, BOB).await.unwrap();
        assert_eq!(alice_cards.len(), 1);
        assert_eq!(alice_cards[0].id, "a::recognition");
        assert_eq!(bob_cards.len(), 1);
        assert_eq!(bob_cards[0].id, "b::recognition");
    }

    #[tokio::test]
    async fn same_card_id_isolated_per_user() {
        // Same id under two users must coexist without one overwriting
        // the other or one user seeing the other's data.
        let db = fresh_db().await;
        let mut alice_card = card("shared::id", Some(100.0));
        alice_card.word = "猫".to_string();
        let mut bob_card = card("shared::id", Some(100.0));
        bob_card.word = "犬".to_string();
        upsert_cards(&db, ALICE, &[alice_card]).await.unwrap();
        upsert_cards(&db, BOB, &[bob_card]).await.unwrap();
        assert_eq!(get_all_cards(&db, ALICE).await.unwrap()[0].word, "猫");
        assert_eq!(get_all_cards(&db, BOB).await.unwrap()[0].word, "犬");
    }

    #[tokio::test]
    async fn otp_store_then_verify_roundtrip() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        // Verify a few seconds later with the same code (handler trims input).
        let ok = verify_otp(&db, ALICE, "012345", now + 5_000).await.unwrap();
        assert!(ok, "fresh code should verify");
    }

    #[tokio::test]
    async fn otp_resend_blocked_while_valid_then_allowed_after_expiry() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "111111", now).await.unwrap());
        // Within the anti-spam floor while the code is still valid: blocked.
        assert!(!store_otp(&db, ALICE, "222222", now + 5_000).await.unwrap());
        // After the floor but code still valid: allowed (rotates the code).
        assert!(store_otp(&db, ALICE, "333333", now + OTP_RESEND_FLOOR_MS)
            .await
            .unwrap());
        // Once the latest code has expired, a resend is always allowed even
        // immediately — the user is never stranded by the TTL.
        let expired_at = now + OTP_RESEND_FLOOR_MS + OTP_TTL_MS + 1;
        assert!(store_otp(&db, ALICE, "444444", expired_at).await.unwrap());
        // And the freshly issued code verifies.
        assert!(verify_otp(&db, ALICE, "444444", expired_at + 1_000)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn otp_wrong_code_fails() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "999999", now + 5_000).await.unwrap());
    }

    #[tokio::test]
    async fn deletions_isolated_per_user() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("x", Some(100.0))])
            .await
            .unwrap();
        upsert_cards(&db, BOB, &[card("x", Some(100.0))])
            .await
            .unwrap();
        // Alice deletes; Bob's copy must survive.
        apply_deletions(&db, ALICE, &["x".to_string()], 1_000)
            .await
            .unwrap();
        assert!(get_all_cards(&db, ALICE).await.unwrap().is_empty());
        assert_eq!(get_all_cards(&db, BOB).await.unwrap().len(), 1);
        assert_eq!(
            get_all_deletions(&db, ALICE).await.unwrap(),
            vec!["x".to_string()]
        );
        assert!(get_all_deletions(&db, BOB).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reviewed_card_not_clobbered_by_unreviewed_copy() {
        // The production bug: a reviewed card (future due_ms, last_review set)
        // was reverted by a stale never-reviewed copy because the merge key was
        // dropped (float parsed with as_i64 → NULL). With last_review_ms as a
        // real REAL column, the unreviewed copy (NULL → 0) must lose.
        let db = fresh_db().await;
        let mut reviewed = card("猫::recognition", Some(1_779_000_000_000.0));
        reviewed.due_ms = 1_780_000_000_000.0; // scheduled into the future
        upsert_cards(&db, ALICE, &[reviewed]).await.unwrap();

        let mut stale = card("猫::recognition", None);
        stale.due_ms = 1_700_000_000_000.0; // older, "due now" copy
        upsert_cards(&db, ALICE, &[stale]).await.unwrap();

        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].last_review_ms, Some(1_779_000_000_000.0));
        assert_eq!(
            stored[0].due_ms, 1_780_000_000_000.0,
            "reviewed schedule must survive a stale unreviewed push"
        );
    }

    fn settings(updated_ms: f64, retention: f64) -> Settings {
        Settings {
            graduation_reps: 0,
            interval_scale: 1.0,
            max_session_cards: 20,
            request_retention: retention,
            updated_ms,
        }
    }

    #[tokio::test]
    async fn settings_insert_then_read_roundtrip() {
        let db = fresh_db().await;
        assert!(get_settings(&db, ALICE).await.unwrap().is_none());
        upsert_settings(&db, ALICE, &settings(100.0, 0.85))
            .await
            .unwrap();
        let got = get_settings(&db, ALICE).await.unwrap().unwrap();
        assert_eq!(got.request_retention, 0.85);
        assert_eq!(got.updated_ms, 100.0);
    }

    #[tokio::test]
    async fn settings_keep_newer_updated_ms() {
        let db = fresh_db().await;
        upsert_settings(&db, ALICE, &settings(200.0, 0.90))
            .await
            .unwrap();
        // Stale write must lose.
        upsert_settings(&db, ALICE, &settings(100.0, 0.70))
            .await
            .unwrap();
        let got = get_settings(&db, ALICE).await.unwrap().unwrap();
        assert_eq!(got.updated_ms, 200.0);
        assert_eq!(got.request_retention, 0.90);
    }

    #[tokio::test]
    async fn settings_isolated_per_user() {
        let db = fresh_db().await;
        upsert_settings(&db, ALICE, &settings(100.0, 0.80))
            .await
            .unwrap();
        upsert_settings(&db, BOB, &settings(100.0, 0.95))
            .await
            .unwrap();
        assert_eq!(
            get_settings(&db, ALICE).await.unwrap().unwrap().request_retention,
            0.80
        );
        assert_eq!(
            get_settings(&db, BOB).await.unwrap().unwrap().request_retention,
            0.95
        );
    }

    #[tokio::test]
    async fn migrates_legacy_blob_cards_to_columns() {
        // Stand up the old single-`data`-blob layout with the merge key living
        // only inside the JSON (the column was always NULL), then confirm
        // init_schema migrates it into typed columns and recovers last_review_ms.
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE cards (
                 email TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL,
                 last_review_ms INTEGER, PRIMARY KEY (email, id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        let blob = r#"{"id":"猫::recognition","word":"猫","direction":"recognition",
            "due_ms":1780000000000.0,"stability":1.5,"difficulty":2.0,"reps":3,
            "lapses":1,"state":"review","last_review_ms":1779000000000.0,
            "added_ms":1778000000000.0,"status":"active"}"#;
        sqlx::query("INSERT INTO cards (email, id, data, last_review_ms) VALUES (?1, ?2, ?3, NULL)")
            .bind(ALICE)
            .bind("猫::recognition")
            .bind(blob)
            .execute(&pool)
            .await
            .unwrap();

        init_schema(&pool).await.unwrap(); // triggers the blob→columns migration

        let stored = get_all_cards(&pool, ALICE).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].word, "猫");
        assert_eq!(stored[0].reps, 3);
        assert_eq!(stored[0].status, "active");
        assert_eq!(stored[0].due_ms, 1_780_000_000_000.0);
        assert_eq!(
            stored[0].last_review_ms,
            Some(1_779_000_000_000.0),
            "merge key must be recovered from the blob"
        );
    }
}
