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

#[cfg(test)]
mod tests {
    #[test]
    fn hex_is_two_lowercase_digits_per_byte() {
        assert_eq!(super::hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }
}
