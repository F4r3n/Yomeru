//! Shared fixtures for the `db` module's tests.
//!
//! Lives in its own file so each table's module can keep its tests next to the
//! code they cover without duplicating the setup.

use super::schema::init_schema;
use super::*;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

// `:memory:` databases live per-connection in SQLite, so a multi-connection
// pool would see a fresh empty DB on each checkout. Pin to one connection.
pub async fn fresh_db() -> Db {
    single_conn_mem().await
}

pub async fn single_conn_mem() -> Db {
    let opts = SqliteConnectOptions::from_str(":memory:").unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    init_schema(&pool).await.unwrap();
    pool
}

pub const ALICE: &str = "alice@example.com";

pub const BOB: &str = "bob@example.com";

// `added_ms` is non-zero so the tombstone guard has something meaningful to
// compare against, but small enough that it never dominates the review
// timestamps in `normalize_version`. `updated_ms` is deliberately left at 0
// so the common fixture exercises that fallback — the path a
// pre-`updated_ms` client takes. Tests about the merge key itself, or about
// a re-add beating a tombstone, set the relevant field explicitly.
pub const ADDED: f64 = 1.0;

/// Server "now" for tests. Far enough ahead that no fixture timestamp trips
/// the fast-clock clamp in `normalize_version`.
pub const NOW: i64 = 1_900_000_000_000;

pub fn del(id: &str, deleted_at: i64) -> Deletion {
    Deletion {
        id: id.to_string(),
        deleted_at,
    }
}

/// Tombstone ids, sorted — the table has no inherent row order.
pub async fn deleted_ids(db: &Db, email: &str) -> Vec<String> {
    let mut ids = get_all_deletions(db, email).await.unwrap();
    ids.sort();
    ids
}

pub fn card(id: &str, last_review_ms: Option<f64>) -> Card {
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
        added_ms: ADDED,
        status: "active".to_string(),
        priority: 0,
        updated_ms: 0.0,
    }
}

pub fn settings(updated_ms: f64, retention: f64) -> Settings {
    Settings {
        graduation_interval_days: 0,
        interval_scale: 1.0,
        max_session_cards: 20,
        request_retention: retention,
        updated_ms,
    }
}

/// When the tombstone for `id` says the delete happened, if there is one.
/// Exposed for tests — the sync response only needs the ids.
pub async fn deleted_at(db: &Db, email: &str, id: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT deleted_at FROM deletions WHERE email = ?1 AND id = ?2")
        .bind(email)
        .bind(id)
        .fetch_optional(db)
        .await
        .unwrap()
}
