//! Table definitions and the schema shims that run at startup.
//!
//! There is no migration framework. Tables are created with `CREATE TABLE IF
//! NOT EXISTS`, and a shape change is handled by one of the `drop_stale_*`
//! helpers dropping the table so it gets recreated. That is safe for `cards`
//! and `settings` because both only mirror state the clients hold and re-upload;
//! `otps`/`sessions` are short-lived by nature.

use anyhow::Context;
use sqlx::SqlitePool;

const SETTINGS_DDL: &str = "CREATE TABLE IF NOT EXISTS settings (
             email                     TEXT PRIMARY KEY,
             graduation_interval_days  INTEGER NOT NULL,
             interval_scale            REAL NOT NULL,
             max_session_cards         INTEGER NOT NULL,
             request_retention         REAL NOT NULL,
             updated_ms                REAL NOT NULL
         )";

/// New per-column `cards` schema. `updated_ms` is the sync merge key;
/// `last_review_ms` is scheduling data and is nullable (never-reviewed cards
/// have no value). Everything else is required.
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
             updated_ms      REAL NOT NULL DEFAULT 0,
             PRIMARY KEY (email, id)
         )";

/// Pending one-time codes, one row per email. `attempts` counts failed guesses
/// so a code can be burned before its TTL runs out.
const OTPS_DDL: &str = "CREATE TABLE IF NOT EXISTS otps (
             email           TEXT PRIMARY KEY,
             code            TEXT NOT NULL,
             expires_at      INTEGER NOT NULL,
             last_requested  INTEGER NOT NULL,
             attempts        INTEGER NOT NULL DEFAULT 0
         )";

/// Live sessions, keyed on the token's hash so a leaked database copy yields no
/// usable bearer tokens.
const SESSIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS sessions (
             token_hash  TEXT PRIMARY KEY,
             email       TEXT NOT NULL,
             expires_at  INTEGER NOT NULL
         )";

/// Tombstones. `deleted_at` is the client's own delete time (clamped to server
/// time on receipt), which is what orders a delete against a later re-add.
const DELETIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS deletions (
             email       TEXT NOT NULL,
             id          TEXT NOT NULL,
             deleted_at  INTEGER NOT NULL,
             PRIMARY KEY (email, id)
         )";

pub(super) async fn init_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    drop_stale_cards(pool).await?;
    drop_stale_settings(pool).await?;
    drop_plaintext_sessions(pool).await?;
    for s in [
        CARDS_DDL,
        SETTINGS_DDL,
        OTPS_DDL,
        SESSIONS_DDL,
        DELETIONS_DDL,
    ] {
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
/// current shape. This has covered three breaks so far — the move off a
/// surface-`word`/`data`-blob key to JMdict `sequence`, the addition of
/// `priority`, and now the addition of the `updated_ms` merge key — and will
/// cover future ones the same way: no data migration, users re-import via
/// export/import. Dropping costs nothing here beyond one re-upload: the table
/// is only a mirror of what clients hold locally, and every client pushes its
/// full card set on each sync. Checking for the newest known column
/// (`updated_ms`) is sufficient on its own, since a table missing it is also
/// missing everything older (`sequence` included). No-op on a fresh DB or one
/// already on the current layout.
async fn drop_stale_cards(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
        .fetch_all(pool)
        .await
        .context("inspect cards columns")?;
    if cols.is_empty() || cols.iter().any(|c| c == "updated_ms") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::cards::{get_all_cards, upsert_cards};
    use crate::db::settings::{get_settings, upsert_settings};
    use crate::db::test_support::*;
    use crate::db::{validate_session, verify_otp};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

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
        upsert_cards(&pool, ALICE, &[card("猫::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        assert_eq!(get_all_cards(&pool, ALICE).await.unwrap().len(), 1);
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

        upsert_cards(&pool, ALICE, &[card("猫::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        assert_eq!(get_all_cards(&pool, ALICE).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn drops_pre_updated_ms_cards_table() {
        // A DB from before `updated_ms` became the merge key. Same clean break
        // as the two above: the table only mirrors what clients hold, and every
        // client re-uploads its full set on the next sync.
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
                 status TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (email, id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO cards
                 (email, id, sequence, direction, due_ms, stability, difficulty,
                  reps, lapses, state, added_ms, status, priority)
             VALUES (?1, '猫::recognition', 1467640, 'recognition', 0, 0, 0, 0, 0, 'new', 0, 'active', 0)",
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
        assert!(cols.iter().any(|c| c == "updated_ms"));
        assert!(
            get_all_cards(&pool, ALICE).await.unwrap().is_empty(),
            "pre-updated_ms row must be dropped, not migrated"
        );

        upsert_cards(&pool, ALICE, &[card("猫::recognition", Some(100.0))], NOW)
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
