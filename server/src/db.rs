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
    /// JMdict ent_seq of the entry this card reviews. Stable across dictionary
    /// rebuilds, unlike the surface string the card used to key on.
    pub sequence: i64,
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
    #[serde(default)]
    pub priority: i64,
}

/// A user's synced scheduler settings. One row per email. `updated_ms` is the
/// last-write-wins merge key (wall-clock ms of the client edit that produced
/// these values). Device-local fields (server URL/email/token) are never
/// stored here — only the knobs that affect scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub graduation_interval_days: i64,
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
             email                     TEXT PRIMARY KEY,
             graduation_interval_days  INTEGER NOT NULL,
             interval_scale            REAL NOT NULL,
             max_session_cards         INTEGER NOT NULL,
             request_retention         REAL NOT NULL,
             updated_ms                REAL NOT NULL
         )";

/// New per-column `cards` schema. `last_review_ms` is the sync merge key and is
/// nullable (never-reviewed cards have no value); everything else is required.
const CARDS_DDL: &str = "CREATE TABLE IF NOT EXISTS cards (
             email           TEXT NOT NULL,
             id              TEXT NOT NULL,
             sequence        INTEGER NOT NULL,
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
             priority        INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (email, id)
         )";

// Minimum gap between OTP emails for the same address — anti-spam only. A new
// code is always allowed once the previous one has expired, so a user who
// misses the TTL is never stranded waiting out a long cooldown.
const OTP_RESEND_FLOOR_MS: i64 = 60_000;
// 10-minute TTL for a generated OTP.
const OTP_TTL_MS: i64 = 600_000;
// Wrong guesses allowed before the code is burned. A 6-digit code is only ~20
// bits, so the TTL alone is not a meaningful brute-force barrier: an attacker
// spread across enough source addresses to defeat the per-IP rate limit could
// otherwise keep guessing for the full 10 minutes. Five attempts caps the odds
// of hitting a given code at 5-in-a-million per issued code.
const OTP_MAX_ATTEMPTS: i64 = 5;

/// Hex-encoded SHA-256, used to keep session tokens out of the database in
/// recoverable form. The token itself is 256 bits of CSPRNG output, so a plain
/// digest is enough — there is no low-entropy input here to make brute-forcing
/// a stolen hash worthwhile, and no salt/KDF is needed.
fn token_hash(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

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
    drop_stale_cards(pool).await?;
    drop_stale_settings(pool).await?;
    drop_plaintext_sessions(pool).await?;
    let stmts = [
        CARDS_DDL,
        SETTINGS_DDL,
        "CREATE TABLE IF NOT EXISTS otps (
             email           TEXT PRIMARY KEY,
             code            TEXT NOT NULL,
             expires_at      INTEGER NOT NULL,
             last_requested  INTEGER NOT NULL,
             attempts        INTEGER NOT NULL DEFAULT 0
         )",
        "CREATE TABLE IF NOT EXISTS sessions (
             token_hash  TEXT PRIMARY KEY,
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
    add_otp_attempts_column(pool).await?;
    Ok(())
}

/// Adds `otps.attempts` to a database created before failed-guess counting
/// existed. `CREATE TABLE IF NOT EXISTS` won't alter an existing table, so the
/// column has to be added explicitly; no-op once present.
async fn add_otp_attempts_column(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('otps')")
        .fetch_all(pool)
        .await
        .context("inspect otps columns")?;
    if cols.iter().any(|c| c == "attempts") {
        return Ok(());
    }
    sqlx::query("ALTER TABLE otps ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .context("add otps.attempts column")?;
    Ok(())
}

/// Drops a `sessions` table that still stores raw bearer tokens so
/// `init_schema` can recreate it keyed on a hash.
///
/// Deliberately destructive: the point of the change is that a database copy
/// must not hand over live sessions, and keeping the old rows around would
/// defeat that. Every user re-authenticates once. No-op on a fresh DB or one
/// already on the hashed layout.
async fn drop_plaintext_sessions(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('sessions')")
        .fetch_all(pool)
        .await
        .context("inspect sessions columns")?;
    if cols.is_empty() || cols.iter().any(|c| c == "token_hash") {
        return Ok(());
    }
    sqlx::query("DROP TABLE sessions")
        .execute(pool)
        .await
        .context("drop plaintext sessions table")?;
    Ok(())
}

/// Drops any stale `cards` table so `init_schema` can recreate it in the
/// current shape. This has covered two breaks so far — the move off a
/// surface-`word`/`data`-blob key to JMdict `sequence`, and now the
/// addition of `priority` — and will cover future ones the same way: no
/// data migration, users re-import via export/import. Checking for the
/// newest known column (`priority`) is sufficient on its own, since a
/// table missing it is also missing everything older (`sequence`
/// included). No-op on a fresh DB or one already on the current layout.
async fn drop_stale_cards(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
        .fetch_all(pool)
        .await
        .context("inspect cards columns")?;
    if cols.is_empty() || cols.iter().any(|c| c == "priority") {
        return Ok(()); // fresh DB or already on the current layout
    }
    sqlx::query("DROP TABLE cards")
        .execute(pool)
        .await
        .context("drop stale cards table")?;
    Ok(())
}

/// Drops any stale `settings` table so `init_schema` can recreate it in the
/// current shape — same rename-by-drop-and-recreate approach as
/// `drop_stale_cards` (`graduation_reps` → `graduation_interval_days`). Safe
/// because `get_settings` already treats a missing row as "client keeps its
/// local settings"; no data migration needed, just a re-sync on next contact.
/// No-op on a fresh DB or one already on the current layout.
async fn drop_stale_settings(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('settings')")
        .fetch_all(pool)
        .await
        .context("inspect settings columns")?;
    if cols.is_empty() || cols.iter().any(|c| c == "graduation_interval_days") {
        return Ok(()); // fresh DB or already on the current layout
    }
    sqlx::query("DROP TABLE settings")
        .execute(pool)
        .await
        .context("drop stale settings table")?;
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
    // A freshly issued code starts with a full attempt budget.
    sqlx::query(
        "INSERT OR REPLACE INTO otps (email, code, expires_at, last_requested, attempts)
         VALUES (?1, ?2, ?3, ?4, 0)",
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

/// Validates code and expiry, deleting the OTP row on success.
///
/// A wrong guess increments `attempts` and burns the code once it reaches
/// [`OTP_MAX_ATTEMPTS`], so a code cannot be ground down over its full TTL.
/// The comparison is constant-time: a byte-by-byte early exit would leak the
/// correct prefix through response timing, turning 10^6 guesses into ~60.
pub async fn verify_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> anyhow::Result<bool> {
    use subtle::ConstantTimeEq;

    let mut tx = db.begin().await.context("begin verify_otp tx")?;
    let row: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT code, expires_at, attempts FROM otps WHERE email = ?1")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await
            .context("read otp")?;

    let ok = match row {
        Some((stored, exp, attempts)) => {
            let live = now_ms < exp && attempts < OTP_MAX_ATTEMPTS;
            let matches: bool = stored.as_bytes().ct_eq(code.as_bytes()).into();
            if live && matches {
                sqlx::query("DELETE FROM otps WHERE email = ?1")
                    .bind(email)
                    .execute(&mut *tx)
                    .await
                    .context("delete otp on verify")?;
                true
            } else {
                // Burn the code outright once the budget is spent, so a stale
                // row can't be retried after the counter maxes out.
                if attempts + 1 >= OTP_MAX_ATTEMPTS {
                    sqlx::query("DELETE FROM otps WHERE email = ?1")
                        .bind(email)
                        .execute(&mut *tx)
                        .await
                        .context("delete exhausted otp")?;
                } else {
                    sqlx::query("UPDATE otps SET attempts = attempts + 1 WHERE email = ?1")
                        .bind(email)
                        .execute(&mut *tx)
                        .await
                        .context("increment otp attempts")?;
                }
                false
            }
        }
        None => false,
    };

    tx.commit().await.context("commit verify_otp tx")?;
    Ok(ok)
}

/// Stores a session keyed on the token's hash. The raw token is returned to the
/// client and never written down, so a leaked database copy yields no usable
/// bearer tokens.
pub async fn create_session(
    db: &Db,
    token: &str,
    email: &str,
    expires_at: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO sessions (token_hash, email, expires_at) VALUES (?1, ?2, ?3)",
    )
    .bind(token_hash(token))
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
        sqlx::query_scalar("SELECT email FROM sessions WHERE token_hash = ?1 AND expires_at > ?2")
            .bind(token_hash(token))
            .bind(now_ms)
            .fetch_optional(db)
            .await
            .context("validate session")?;
    Ok(email)
}

/// Deletes expired sessions and OTPs. Both tables are append-mostly — nothing
/// removed them before, so they grew without bound for the life of the server.
pub async fn prune_expired_auth(db: &Db, now_ms: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?1")
        .bind(now_ms)
        .execute(db)
        .await
        .context("prune expired sessions")?;
    sqlx::query("DELETE FROM otps WHERE expires_at <= ?1")
        .bind(now_ms)
        .execute(db)
        .await
        .context("prune expired otps")?;
    Ok(())
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
                 (email, id, sequence, direction, due_ms, stability, difficulty,
                  reps, lapses, state, last_review_ms, added_ms, status, priority)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(email, id) DO UPDATE SET
                 sequence = excluded.sequence,
                 direction = excluded.direction,
                 due_ms = excluded.due_ms,
                 stability = excluded.stability,
                 difficulty = excluded.difficulty,
                 reps = excluded.reps,
                 lapses = excluded.lapses,
                 state = excluded.state,
                 last_review_ms = excluded.last_review_ms,
                 added_ms = excluded.added_ms,
                 status = excluded.status,
                 priority = excluded.priority
             WHERE COALESCE(excluded.last_review_ms, 0) >= COALESCE(cards.last_review_ms, 0)",
        )
        .bind(email)
        .bind(&c.id)
        .bind(c.sequence)
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
        .bind(c.priority)
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
        "SELECT id, sequence, direction, due_ms, stability, difficulty,
                reps, lapses, state, last_review_ms, added_ms, status, priority
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
            sequence: r.get("sequence"),
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
            priority: r.get("priority"),
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
             (email, graduation_interval_days, interval_scale, max_session_cards,
              request_retention, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(email) DO UPDATE SET
             graduation_interval_days = excluded.graduation_interval_days,
             interval_scale = excluded.interval_scale,
             max_session_cards = excluded.max_session_cards,
             request_retention = excluded.request_retention,
             updated_ms = excluded.updated_ms
         WHERE excluded.updated_ms >= settings.updated_ms",
    )
    .bind(email)
    .bind(s.graduation_interval_days)
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
        "SELECT graduation_interval_days, interval_scale, max_session_cards,
                request_retention, updated_ms
         FROM settings WHERE email = ?1",
    )
    .bind(email)
    .fetch_optional(db)
    .await
    .context("query get_settings")?;
    Ok(row.map(|r| Settings {
        graduation_interval_days: r.get("graduation_interval_days"),
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
            sequence: 1_467_640,
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
            priority: 0,
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
        newer.sequence = 1_586_270;
        upsert_cards(&db, ALICE, &[newer]).await.unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].sequence, 1_586_270);
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
        alice_card.sequence = 1_467_640;
        let mut bob_card = card("shared::id", Some(100.0));
        bob_card.sequence = 1_586_270;
        upsert_cards(&db, ALICE, &[alice_card]).await.unwrap();
        upsert_cards(&db, BOB, &[bob_card]).await.unwrap();
        assert_eq!(
            get_all_cards(&db, ALICE).await.unwrap()[0].sequence,
            1_467_640
        );
        assert_eq!(
            get_all_cards(&db, BOB).await.unwrap()[0].sequence,
            1_586_270
        );
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
        assert!(
            store_otp(&db, ALICE, "333333", now + OTP_RESEND_FLOOR_MS)
                .await
                .unwrap()
        );
        // Once the latest code has expired, a resend is always allowed even
        // immediately — the user is never stranded by the TTL.
        let expired_at = now + OTP_RESEND_FLOOR_MS + OTP_TTL_MS + 1;
        assert!(store_otp(&db, ALICE, "444444", expired_at).await.unwrap());
        // And the freshly issued code verifies.
        assert!(
            verify_otp(&db, ALICE, "444444", expired_at + 1_000)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn otp_wrong_code_fails() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "999999", now + 5_000).await.unwrap());
    }

    #[tokio::test]
    async fn otp_burns_after_max_failed_attempts() {
        // Without a budget a 6-digit code could be ground down for its whole
        // 10-minute TTL by anyone able to spread requests across enough IPs.
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        for i in 0..OTP_MAX_ATTEMPTS {
            assert!(
                !verify_otp(&db, ALICE, "999999", now + 1_000).await.unwrap(),
                "wrong guess {i} must fail"
            );
        }
        // The code is gone, so even the *right* value no longer works.
        assert!(
            !verify_otp(&db, ALICE, "012345", now + 1_000).await.unwrap(),
            "exhausted code must be burned, not merely rejected"
        );
    }

    #[tokio::test]
    async fn otp_attempts_reset_on_resend() {
        // Burning a code must not leave the user locked out — the next code
        // starts with a fresh budget.
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        for _ in 0..OTP_MAX_ATTEMPTS {
            let _ = verify_otp(&db, ALICE, "999999", now + 1_000).await.unwrap();
        }
        assert!(store_otp(&db, ALICE, "543210", now + 2_000).await.unwrap());
        assert!(verify_otp(&db, ALICE, "543210", now + 3_000).await.unwrap());
    }

    #[tokio::test]
    async fn otp_succeeds_within_attempt_budget() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "111111", now + 1_000).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "222222", now + 1_000).await.unwrap());
        assert!(verify_otp(&db, ALICE, "012345", now + 1_000).await.unwrap());
    }

    #[tokio::test]
    async fn session_token_is_not_stored_in_the_clear() {
        // A database copy must not yield usable bearer tokens.
        let db = fresh_db().await;
        create_session(&db, "supersecrettoken", ALICE, 2_000)
            .await
            .unwrap();
        let stored: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_ne!(stored[0], "supersecrettoken");
        assert_eq!(stored[0], token_hash("supersecrettoken"));
        // ...and the raw token still validates.
        assert_eq!(
            validate_session(&db, "supersecrettoken", 1_000)
                .await
                .unwrap()
                .as_deref(),
            Some(ALICE)
        );
    }

    #[tokio::test]
    async fn session_rejects_wrong_or_expired_token() {
        let db = fresh_db().await;
        create_session(&db, "good", ALICE, 2_000).await.unwrap();
        assert!(validate_session(&db, "bad", 1_000).await.unwrap().is_none());
        assert!(
            validate_session(&db, "good", 3_000)
                .await
                .unwrap()
                .is_none(),
            "expired session must not validate"
        );
    }

    #[tokio::test]
    async fn prune_removes_expired_sessions_and_otps() {
        let db = fresh_db().await;
        create_session(&db, "live", ALICE, 10_000).await.unwrap();
        create_session(&db, "dead", BOB, 1_000).await.unwrap();
        // Issued at t=0, so this code expires at OTP_TTL_MS.
        store_otp(&db, ALICE, "012345", 0).await.unwrap();

        // Cutoff sits after the dead session but before the OTP's TTL.
        prune_expired_auth(&db, 5_000).await.unwrap();

        let count = |table: &'static str| {
            let db = db.clone();
            async move {
                sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                    .fetch_one(&db)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(count("sessions").await, 1, "expired session should be gone");
        assert!(
            validate_session(&db, "live", 4_000)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(count("otps").await, 1, "still-valid otp must survive");

        // Past the OTP TTL it goes too.
        prune_expired_auth(&db, OTP_TTL_MS + 1).await.unwrap();
        assert_eq!(count("otps").await, 0, "expired otp should be gone");
        assert_eq!(count("sessions").await, 0, "all sessions now expired");
    }

    #[tokio::test]
    async fn drops_plaintext_session_table_on_upgrade() {
        // Pre-hash databases stored raw bearer tokens. Those rows are exactly
        // what the change exists to eliminate, so they must not survive.
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE sessions (
                 token TEXT PRIMARY KEY, email TEXT NOT NULL, expires_at INTEGER NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sessions (token, email, expires_at) VALUES ('raw', ?1, 9999)")
            .bind(ALICE)
            .execute(&pool)
            .await
            .unwrap();

        init_schema(&pool).await.unwrap();

        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('sessions')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(cols.iter().any(|c| c == "token_hash"));
        assert!(!cols.iter().any(|c| c == "token"));
        assert!(validate_session(&pool, "raw", 0).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn adds_attempts_column_to_legacy_otps_table() {
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE otps (
                 email TEXT PRIMARY KEY, code TEXT NOT NULL,
                 expires_at INTEGER NOT NULL, last_requested INTEGER NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO otps VALUES (?1, '012345', 9_999_999, 0)")
            .bind(ALICE)
            .execute(&pool)
            .await
            .unwrap();

        init_schema(&pool).await.unwrap();

        // Existing code survives the migration and is still verifiable.
        assert!(verify_otp(&pool, ALICE, "012345", 1_000).await.unwrap());
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
            graduation_interval_days: 0,
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
            get_settings(&db, ALICE)
                .await
                .unwrap()
                .unwrap()
                .request_retention,
            0.80
        );
        assert_eq!(
            get_settings(&db, BOB)
                .await
                .unwrap()
                .unwrap()
                .request_retention,
            0.95
        );
    }

    #[tokio::test]
    async fn drops_legacy_word_keyed_cards_table() {
        // A pre-`sequence` DB keyed cards on a surface `word` string. The move to
        // JMdict ent_seq is a clean break: init_schema must drop that table and
        // recreate it on the `sequence` layout (no migration — users re-import).
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE cards (
                 email TEXT NOT NULL, id TEXT NOT NULL, word TEXT NOT NULL,
                 direction TEXT NOT NULL, due_ms REAL NOT NULL, stability REAL NOT NULL,
                 difficulty REAL NOT NULL, reps INTEGER NOT NULL, lapses INTEGER NOT NULL,
                 state TEXT NOT NULL, last_review_ms REAL, added_ms REAL NOT NULL,
                 status TEXT NOT NULL, PRIMARY KEY (email, id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO cards
                 (email, id, word, direction, due_ms, stability, difficulty,
                  reps, lapses, state, added_ms, status)
             VALUES (?1, '猫::recognition', '猫', 'recognition', 0, 0, 0, 0, 0, 'new', 0, 'active')",
        )
        .bind(ALICE)
        .execute(&pool)
        .await
        .unwrap();

        init_schema(&pool).await.unwrap(); // drops the legacy table, recreates on sequence

        // Table is now on the new layout and empty (legacy rows discarded).
        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(cols.iter().any(|c| c == "sequence"));
        assert!(!cols.iter().any(|c| c == "word"));
        assert!(get_all_cards(&pool, ALICE).await.unwrap().is_empty());

        // And the recreated table accepts sequence-keyed cards.
        upsert_cards(&pool, ALICE, &[card("猫::recognition", Some(100.0))])
            .await
            .unwrap();
        assert_eq!(get_all_cards(&pool, ALICE).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn upsert_and_read_roundtrips_priority() {
        let db = fresh_db().await;
        let mut c = card("a::recognition", Some(100.0));
        c.priority = 5;
        upsert_cards(&db, ALICE, &[c]).await.unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].priority, 5);
    }

    #[tokio::test]
    async fn drops_pre_priority_cards_table() {
        // A DB on the sequence-keyed layout from before `priority` existed.
        // init_schema must drop and recreate it (clean break, not migrated —
        // users re-import), same as the word->sequence break above.
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE cards (
                 email TEXT NOT NULL, id TEXT NOT NULL, sequence INTEGER NOT NULL,
                 direction TEXT NOT NULL, due_ms REAL NOT NULL, stability REAL NOT NULL,
                 difficulty REAL NOT NULL, reps INTEGER NOT NULL, lapses INTEGER NOT NULL,
                 state TEXT NOT NULL, last_review_ms REAL, added_ms REAL NOT NULL,
                 status TEXT NOT NULL, PRIMARY KEY (email, id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO cards
                 (email, id, sequence, direction, due_ms, stability, difficulty,
                  reps, lapses, state, added_ms, status)
             VALUES (?1, '猫::recognition', 1467640, 'recognition', 0, 0, 0, 0, 0, 'new', 0, 'active')",
        )
        .bind(ALICE)
        .execute(&pool)
        .await
        .unwrap();

        init_schema(&pool).await.unwrap();

        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(cols.iter().any(|c| c == "priority"));
        assert!(
            get_all_cards(&pool, ALICE).await.unwrap().is_empty(),
            "pre-priority row must be dropped, not migrated"
        );

        upsert_cards(&pool, ALICE, &[card("猫::recognition", Some(100.0))])
            .await
            .unwrap();
        assert_eq!(get_all_cards(&pool, ALICE).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn drops_pre_graduation_interval_settings_table() {
        // A DB on the old graduation_reps layout. init_schema must drop and
        // recreate it (clean break, not migrated — client re-syncs its local
        // settings on next contact), same approach as the cards-table breaks.
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE settings (
                 email             TEXT PRIMARY KEY,
                 graduation_reps   INTEGER NOT NULL,
                 interval_scale    REAL NOT NULL,
                 max_session_cards INTEGER NOT NULL,
                 request_retention REAL NOT NULL,
                 updated_ms        REAL NOT NULL
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO settings
                 (email, graduation_reps, interval_scale, max_session_cards,
                  request_retention, updated_ms)
             VALUES (?1, 5, 1.0, 20, 0.9, 100)",
        )
        .bind(ALICE)
        .execute(&pool)
        .await
        .unwrap();

        init_schema(&pool).await.unwrap();

        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('settings')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(cols.iter().any(|c| c == "graduation_interval_days"));
        assert!(
            get_settings(&pool, ALICE).await.unwrap().is_none(),
            "pre-migration settings row must be dropped, not migrated"
        );

        upsert_settings(&pool, ALICE, &settings(200.0, 0.9))
            .await
            .unwrap();
        assert!(get_settings(&pool, ALICE).await.unwrap().is_some());
    }
}
