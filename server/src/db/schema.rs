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
             again_count     INTEGER NOT NULL DEFAULT 0,
             updated_ms      REAL NOT NULL DEFAULT 0,
             rev             INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (email, id)
         )";

/// Append-only answer log; see `db::reviews`. The largest table by far, so it
/// is laid out to stay small:
/// - `WITHOUT ROWID` stores rows in primary-key order, so the key isn't kept a
///   second time in a separate index.
/// - `kind` is `0` = review, `1` = drill, and `reviewed_ms` an integer, which
///   SQLite packs tighter than text and 8-byte floats.
/// - `user_id` instead of the email: an integer of a byte or two, not ~25
///   bytes of text on every row.
/// - `kind` leads the key after `user_id`, so counting a user's drills is a range
///   scan over just their drill rows; no extra index needed.
///
/// A row is identified by the card and the client's answer time (a card is
/// never answered as both kinds in the same ms), so a re-sent upload is ignored.
const REVIEWS_DDL: &str = "CREATE TABLE IF NOT EXISTS reviews (
             user_id      INTEGER NOT NULL REFERENCES users (id),
             kind         INTEGER NOT NULL,
             card_id      TEXT NOT NULL,
             reviewed_ms  INTEGER NOT NULL,
             rating       INTEGER NOT NULL,
             rev          INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (user_id, kind, card_id, reviewed_ms)
         ) WITHOUT ROWID";

/// Pending one-time codes, one row per email. `attempts` counts failed guesses
/// so a code can be burned before its TTL runs out.
const OTPS_DDL: &str = "CREATE TABLE IF NOT EXISTS otps (
             email           TEXT PRIMARY KEY,
             code            TEXT NOT NULL,
             expires_at      INTEGER NOT NULL,
             last_requested  INTEGER NOT NULL,
             attempts        INTEGER NOT NULL DEFAULT 0
         )";

/// One row per account, created on first sign-in. The integer id keys the
/// tables that hold many rows per user, where repeating the email would be
/// most of the row. `cards`, `settings` and `deletions` still key on email;
/// `otps` always will, since a code is requested before the account exists.
const USERS_DDL: &str = "CREATE TABLE IF NOT EXISTS users (
             id     INTEGER PRIMARY KEY,
             email  TEXT NOT NULL UNIQUE
         )";

/// Live sessions, keyed on the token's hash so a leaked database copy yields no
/// usable bearer tokens.
const SESSIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS sessions (
             token_hash  TEXT PRIMARY KEY,
             user_id     INTEGER NOT NULL REFERENCES users (id),
             expires_at  INTEGER NOT NULL
         )";

/// Tombstones. `deleted_at` is the client's own delete time (clamped to server
/// time on receipt), which is what orders a delete against a later re-add.
const DELETIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS deletions (
             email       TEXT NOT NULL,
             id          TEXT NOT NULL,
             deleted_at  INTEGER NOT NULL,
             rev         INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (email, id)
         )";

/// The delta-sync clock; see `db::sync_meta`. A single row: `epoch` names this
/// database's `rev` sequence, `rev` is the last revision handed out, and
/// `pruned_rev` the newest revision a pruned tombstone carried.
const SYNC_META_DDL: &str = "CREATE TABLE IF NOT EXISTS sync_meta (
             id          INTEGER PRIMARY KEY CHECK (id = 1),
             epoch       TEXT NOT NULL,
             rev         INTEGER NOT NULL,
             pruned_rev  INTEGER NOT NULL
         )";

pub(super) async fn init_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    drop_stale_cards(pool).await?;
    drop_stale_settings(pool).await?;
    drop_plaintext_sessions(pool).await?;
    drop_email_keyed_reviews(pool).await?;
    sqlx::query(USERS_DDL)
        .execute(pool)
        .await
        .context("create users table")?;
    backfill_users(pool).await?;
    move_sessions_to_user_id(pool).await?;
    for s in [
        CARDS_DDL,
        SETTINGS_DDL,
        OTPS_DDL,
        SESSIONS_DDL,
        DELETIONS_DDL,
        REVIEWS_DDL,
        SYNC_META_DDL,
    ] {
        sqlx::query(s)
            .execute(pool)
            .await
            .context("init db schema")?;
    }
    add_otp_attempts_column(pool).await?;
    add_cards_again_count_column(pool).await?;
    for table in ["cards", "deletions", "reviews"] {
        add_rev_column(pool, table).await?;
    }
    for ddl in [
        "CREATE INDEX IF NOT EXISTS cards_by_rev ON cards (email, rev)",
        "CREATE INDEX IF NOT EXISTS deletions_by_rev ON deletions (email, rev)",
    ] {
        sqlx::query(ddl)
            .execute(pool)
            .await
            .context("create rev index")?;
    }
    super::sync_meta::seed(pool).await?;
    Ok(())
}

/// Adds the delta-sync `rev` column to a table created before delta sync.
/// Existing rows read as revision 0. That loses nothing: no client holds a
/// cursor yet, so each one's first sync after the upgrade is a full one.
/// No-op once present.
async fn add_rev_column(pool: &SqlitePool, table: &str) -> anyhow::Result<()> {
    if columns(pool, table).await?.iter().any(|c| c == "rev") {
        return Ok(());
    }
    sqlx::query(&format!(
        "ALTER TABLE {table} ADD COLUMN rev INTEGER NOT NULL DEFAULT 0"
    ))
    .execute(pool)
    .await
    .with_context(|| format!("add {table}.rev column"))?;
    Ok(())
}

/// Adds `cards.again_count` to a database created before failure counting
/// existed. Added in place rather than via `drop_stale_cards`: the column has
/// a sane default, so there is no reason to make every client re-upload.
/// No-op once present.
async fn add_cards_again_count_column(pool: &SqlitePool) -> anyhow::Result<()> {
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
        .fetch_all(pool)
        .await
        .context("inspect cards columns")?;
    if cols.iter().any(|c| c == "again_count") {
        return Ok(());
    }
    sqlx::query("ALTER TABLE cards ADD COLUMN again_count INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .context("add cards.again_count column")?;
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

/// Column names of `table`; empty if it doesn't exist.
async fn columns(pool: &SqlitePool, table: &str) -> anyhow::Result<Vec<String>> {
    sqlx::query_scalar("SELECT name FROM pragma_table_info(?1)")
        .bind(table)
        .fetch_all(pool)
        .await
        .with_context(|| format!("inspect {table} columns"))
}

/// Drops a `reviews` table from the short-lived build that keyed it on email.
/// That build was never deployed, so this only clears dev databases; the rows
/// are expendable test data. No-op on a fresh DB or the current layout.
async fn drop_email_keyed_reviews(pool: &SqlitePool) -> anyhow::Result<()> {
    if columns(pool, "reviews").await?.iter().any(|c| c == "email") {
        sqlx::query("DROP TABLE reviews")
            .execute(pool)
            .await
            .context("drop email-keyed reviews table")?;
    }
    Ok(())
}

/// Gives every account that predates the `users` table an id: anyone with a
/// session, cards or settings. Runs on every startup and is a no-op once they
/// all exist, which also covers a database restored from an older backup.
async fn backfill_users(pool: &SqlitePool) -> anyhow::Result<()> {
    let sessions_have_email = columns(pool, "sessions")
        .await?
        .iter()
        .any(|c| c == "email");
    let mut sources = vec!["cards", "settings"];
    if sessions_have_email {
        sources.push("sessions");
    }
    for table in sources {
        if columns(pool, table).await?.is_empty() {
            continue;
        }
        sqlx::query(&format!(
            "INSERT OR IGNORE INTO users (email) SELECT DISTINCT email FROM {table}"
        ))
        .execute(pool)
        .await
        .with_context(|| format!("backfill users from {table}"))?;
    }
    Ok(())
}

/// Rewrites an email-keyed `sessions` table onto `user_id`, keeping every live
/// session so nobody is signed out by the upgrade. Runs after
/// [`backfill_users`], so every session's email has an id to join on. No-op on
/// a fresh DB or one already on the current layout.
async fn move_sessions_to_user_id(pool: &SqlitePool) -> anyhow::Result<()> {
    if !columns(pool, "sessions")
        .await?
        .iter()
        .any(|c| c == "email")
    {
        return Ok(());
    }
    let mut tx = pool.begin().await.context("begin sessions migration")?;
    sqlx::query("ALTER TABLE sessions RENAME TO sessions_by_email")
        .execute(&mut *tx)
        .await
        .context("rename email-keyed sessions")?;
    sqlx::query(SESSIONS_DDL)
        .execute(&mut *tx)
        .await
        .context("create user-keyed sessions")?;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         SELECT s.token_hash, u.id, s.expires_at
         FROM sessions_by_email s JOIN users u ON u.email = s.email",
    )
    .execute(&mut *tx)
    .await
    .context("copy sessions onto user ids")?;
    sqlx::query("DROP TABLE sessions_by_email")
        .execute(&mut *tx)
        .await
        .context("drop email-keyed sessions")?;
    tx.commit().await.context("commit sessions migration")?;
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
/// is only a mirror of what clients hold locally, and a client pushes its full
/// card set on a full sync. Delta clients only upload what they changed, so
/// the drop also rotates the sync epoch, which turns every client's next sync
/// back into a full one. Checking for the newest known column
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
    if !columns(pool, "sync_meta").await?.is_empty() {
        sqlx::query("UPDATE sync_meta SET epoch = lower(hex(randomblob(8)))")
            .execute(pool)
            .await
            .context("rotate sync epoch after dropping cards")?;
    }
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
    async fn moves_email_keyed_sessions_onto_user_ids_without_signing_anyone_out() {
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        // The layout before `users` existed: sessions and cards keyed on email.
        sqlx::query(
            "CREATE TABLE sessions (
                 token_hash TEXT PRIMARY KEY, email TEXT NOT NULL, expires_at INTEGER NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sessions VALUES (?1, ?2, 9999)")
            .bind(crate::db::auth::token_hash("alice-token"))
            .bind(ALICE)
            .execute(&pool)
            .await
            .unwrap();
        init_schema(&pool).await.unwrap(); // creates cards on the current layout
        upsert_cards(
            &pool,
            BOB,
            &[card("1467640::recognition", Some(100.0))],
            NOW,
        )
        .await
        .unwrap();
        // Idempotent, and picks up Bob, who has cards but no session.
        init_schema(&pool).await.unwrap();

        let alice = validate_session(&pool, "alice-token", 0)
            .await
            .unwrap()
            .expect("session survives the upgrade");
        assert_eq!(alice.email, ALICE);
        let users: Vec<String> = sqlx::query_scalar("SELECT email FROM users ORDER BY email")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(users, vec![ALICE.to_string(), BOB.to_string()]);
        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('sessions')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(cols.iter().any(|c| c == "user_id"));
        assert!(!cols.iter().any(|c| c == "email"));
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
    async fn adds_again_count_to_an_existing_cards_table_without_dropping_rows() {
        // Unlike the breaks above, `again_count` is added in place: a card
        // stored before the column existed keeps its row and reads back as 0.
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
                 updated_ms REAL NOT NULL DEFAULT 0,
                 PRIMARY KEY (email, id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO cards
                 (email, id, sequence, direction, due_ms, stability, difficulty,
                  reps, lapses, state, added_ms, status, priority, updated_ms)
             VALUES (?1, '1467640::recognition', 1467640, 'recognition', 0, 0, 0, 0, 0,
                     'new', 0, 'active', 0, 5)",
        )
        .bind(ALICE)
        .execute(&pool)
        .await
        .unwrap();

        init_schema(&pool).await.unwrap();
        // Idempotent: a second startup must not try to add the column again.
        init_schema(&pool).await.unwrap();

        let cards = get_all_cards(&pool, ALICE).await.unwrap();
        assert_eq!(cards.len(), 1, "existing row must survive the upgrade");
        assert_eq!(cards[0].again_count, 0);
    }

    #[tokio::test]
    async fn adds_rev_to_tables_from_before_delta_sync_without_dropping_rows() {
        let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE deletions (
                 email TEXT NOT NULL, id TEXT NOT NULL, deleted_at INTEGER NOT NULL,
                 PRIMARY KEY (email, id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO deletions VALUES (?1, 'a::recognition', 5)")
            .bind(ALICE)
            .execute(&pool)
            .await
            .unwrap();

        init_schema(&pool).await.unwrap();
        init_schema(&pool).await.unwrap();

        for table in ["cards", "deletions", "reviews"] {
            assert!(
                columns(&pool, table)
                    .await
                    .unwrap()
                    .iter()
                    .any(|c| c == "rev"),
                "{table} has no rev column"
            );
        }
        let rev: i64 = sqlx::query_scalar("SELECT rev FROM deletions WHERE id = 'a::recognition'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rev, 0, "existing rows survive at revision 0");
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
