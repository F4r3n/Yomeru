use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration;

pub type Db = SqlitePool;

// 1-hour cooldown between OTP requests for the same email.
const OTP_COOLDOWN_MS: i64 = 3_600_000;
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
    let stmts = [
        "CREATE TABLE IF NOT EXISTS cards (
             email           TEXT NOT NULL,
             id              TEXT NOT NULL,
             data            TEXT NOT NULL,
             last_review_ms  INTEGER,
             PRIMARY KEY (email, id)
         )",
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

/// `Ok(true)` = stored, `Ok(false)` = still in 1-hour cooldown, `Err` = db error.
///
/// SELECT + INSERT are wrapped in a transaction so concurrent OTP requests
/// for the same email can't both pass the cooldown check.
pub async fn store_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> anyhow::Result<bool> {
    let mut tx = db.begin().await.context("begin store_otp tx")?;
    let last: Option<i64> = sqlx::query_scalar("SELECT last_requested FROM otps WHERE email = ?1")
        .bind(email)
        .fetch_optional(&mut *tx)
        .await
        .context("read otp cooldown")?;
    if let Some(t) = last {
        if now_ms - t < OTP_COOLDOWN_MS {
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
pub async fn upsert_cards(
    db: &Db,
    email: &str,
    cards: &[serde_json::Value],
) -> anyhow::Result<()> {
    let mut tx = db.begin().await.context("begin upsert tx")?;
    for card in cards {
        let id = match card.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let last_review_ms = card.get("last_review_ms").and_then(|v| v.as_i64());
        let data = serde_json::to_string(card).unwrap_or_default();
        sqlx::query(
            "INSERT INTO cards (email, id, data, last_review_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(email, id) DO UPDATE SET
                 data = excluded.data,
                 last_review_ms = excluded.last_review_ms
             WHERE COALESCE(excluded.last_review_ms, 0) >= COALESCE(cards.last_review_ms, 0)",
        )
        .bind(email)
        .bind(id)
        .bind(&data)
        .bind(last_review_ms)
        .execute(&mut *tx)
        .await
        .context("upsert card")?;
        sqlx::query("DELETE FROM deletions WHERE email = ?1 AND id = ?2")
            .bind(email)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("clear matching tombstone")?;
    }
    tx.commit().await.context("commit upsert tx")?;
    Ok(())
}

pub async fn get_all_cards(db: &Db, email: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT data FROM cards WHERE email = ?1")
        .bind(email)
        .fetch_all(db)
        .await
        .context("query get_all_cards")?;
    Ok(rows
        .into_iter()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect())
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
    use serde_json::json;

    // `:memory:` databases live per-connection in SQLite, so a multi-connection
    // pool would see a fresh empty DB on each checkout. Pin to one connection.
    async fn fresh_db() -> Db {
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

    fn card(id: &str, last_review_ms: Option<i64>) -> serde_json::Value {
        json!({ "id": id, "last_review_ms": last_review_ms, "word": "猫" })
    }

    #[tokio::test]
    async fn upsert_inserts_new_cards() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100))])
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["id"], "a::recognition");
    }

    #[tokio::test]
    async fn upsert_keeps_newer_last_review() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100))])
            .await
            .unwrap();
        // Older incoming write should be ignored.
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(50))])
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0]["last_review_ms"], 100);
    }

    #[tokio::test]
    async fn upsert_replaces_with_equal_or_newer_last_review() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100))])
            .await
            .unwrap();
        let mut newer = card("a::recognition", Some(200));
        newer["word"] = json!("犬");
        upsert_cards(&db, ALICE, &[newer]).await.unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0]["word"], "犬");
        assert_eq!(stored[0]["last_review_ms"], 200);
    }

    #[tokio::test]
    async fn apply_deletions_removes_card_and_records_tombstone() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100))])
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
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(2_000))])
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
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100))])
            .await
            .unwrap();
        upsert_cards(&db, BOB, &[card("b::recognition", Some(200))])
            .await
            .unwrap();
        let alice_cards = get_all_cards(&db, ALICE).await.unwrap();
        let bob_cards = get_all_cards(&db, BOB).await.unwrap();
        assert_eq!(alice_cards.len(), 1);
        assert_eq!(alice_cards[0]["id"], "a::recognition");
        assert_eq!(bob_cards.len(), 1);
        assert_eq!(bob_cards[0]["id"], "b::recognition");
    }

    #[tokio::test]
    async fn same_card_id_isolated_per_user() {
        // Same id under two users must coexist without one overwriting
        // the other or one user seeing the other's data.
        let db = fresh_db().await;
        let mut alice_card = card("shared::id", Some(100));
        alice_card["word"] = json!("猫");
        let mut bob_card = card("shared::id", Some(100));
        bob_card["word"] = json!("犬");
        upsert_cards(&db, ALICE, &[alice_card]).await.unwrap();
        upsert_cards(&db, BOB, &[bob_card]).await.unwrap();
        assert_eq!(
            get_all_cards(&db, ALICE).await.unwrap()[0]["word"],
            "猫"
        );
        assert_eq!(get_all_cards(&db, BOB).await.unwrap()[0]["word"], "犬");
    }

    #[tokio::test]
    async fn deletions_isolated_per_user() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("x", Some(100))])
            .await
            .unwrap();
        upsert_cards(&db, BOB, &[card("x", Some(100))]).await.unwrap();
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
}
