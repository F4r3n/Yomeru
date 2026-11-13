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
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::str::FromStr;
use std::time::Duration;

mod auth;
mod cards;
mod deletions;
mod reviews;
mod schema;
mod settings;
mod sync_meta;
#[cfg(test)]
pub(crate) mod test_support;

/// Lowercase hex, two digits per byte.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|b| [b >> 4, b & 0x0f])
        .filter_map(|n| char::from_digit(u32::from(n), 16))
        .collect()
}

pub use auth::{
    SessionUser, create_session, delete_account, prune_expired_auth, store_otp, validate_session,
    verify_otp,
};
pub use cards::{Card, get_all_cards, get_cards_since, upsert_cards_tx};
pub use deletions::{
    Deletion, DeletionEntry, apply_deletions_tx, get_all_deletions, get_deletions_since,
    prune_old_deletions,
};
pub use reviews::{ReviewLog, get_drill_counts, get_drill_counts_since, insert_reviews_tx};
pub use settings::{Settings, get_settings, upsert_settings};
pub use sync_meta::{Cursor, next_rev_tx};

pub type Db = SqlitePool;

/// The two pools over one database file.
pub struct Pools {
    pub db: Db,
    /// Single connection for sync transactions. Every sync takes the write lock,
    /// so concurrent syncs queue here in order instead of spinning on SQLite's
    /// busy handler, which is unfair and timed out under bursts.
    pub sync_db: Db,
}

pub async fn init_db(path: &str) -> anyhow::Result<Pools> {
    let opts = SqliteConnectOptions::from_str(path)
        .with_context(|| format!("parse sqlite path {path}"))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        // Safe with WAL: a power loss can drop the last commits, never corrupt.
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    let db = SqlitePoolOptions::new()
        .connect_with(opts.clone())
        .await
        .with_context(|| format!("open sqlite db at {path}"))?;
    schema::init_schema(&db).await?;
    let sync_db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .with_context(|| format!("open sqlite sync pool at {path}"))?;
    Ok(Pools { db, sync_db })
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_is_two_lowercase_digits_per_byte() {
        assert_eq!(super::hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }
}
