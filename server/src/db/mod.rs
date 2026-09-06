//! SQLite persistence, split by table.
//!
//! Everything is re-exported here, so callers use `db::Card`, `db::upsert_cards`
//! and so on without caring which file an item lives in.
//!
//! The two halves of the sync merge — [`cards`] and [`deletions`] — are the ones
//! worth reading together: each guards against the other's writes, and the
//! guards only work as a pair.

use anyhow::Context;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;

mod auth;
mod cards;
mod deletions;
mod schema;
mod settings;
#[cfg(test)]
pub(crate) mod test_support;

pub use auth::{create_session, prune_expired_auth, store_otp, validate_session, verify_otp};
pub use cards::{Card, get_all_cards, upsert_cards_tx};
pub use deletions::{
    Deletion, DeletionEntry, apply_deletions_tx, get_all_deletions, prune_old_deletions,
};
pub use settings::{Settings, get_settings, upsert_settings};

pub type Db = SqlitePool;

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
    schema::init_schema(&pool).await?;
    Ok(pool)
}
