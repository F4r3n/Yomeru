//! The delta-sync clock.
//!
//! Every `/api/sync` transaction takes the next revision ([`next_rev_tx`]) and
//! stamps it on each card, tombstone and review row it writes. A client keeps
//! the revision its last response ended at (its cursor) and next time asks
//! only for rows stamped after it. SQLite runs one writer at a time, so
//! revisions are handed out in commit order and a cursor never skips a row
//! committed before it.
//!
//! A cursor is only meaningful against the sequence it came from and for the
//! user it was issued to, so its `epoch` names both: the database's epoch and
//! the user id. A cursor from another epoch (the database was recreated, or
//! rotated after a backup restore or a cards-table drop) or another user (a
//! sign-in raced a sync still in flight for the previous account) forces a
//! full sync. A client that sent a cursor and gets a full answer re-uploads
//! its whole deck, which is what repopulates a server that lost rows.
//!
//! Restoring a backup rolls `rev` back behind what clients have seen, and a
//! cursor that lands between the restored `rev` and later writes would miss
//! those writes. So a restore must rotate the epoch (see `scripts/deploy.sh`):
//! `UPDATE sync_meta SET epoch = lower(hex(randomblob(8)))`.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// Where a client's view of the server ends. Opaque to clients: they store it
/// and send it back. `epoch` is `"<database epoch>.<user id>"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub epoch: String,
    pub rev: i64,
}

/// The clock's state as seen by one sync transaction.
pub struct SyncClock {
    pub epoch: String,
    /// The revision this transaction stamps on its writes.
    pub rev: i64,
    /// Newest revision carried by a tombstone that has since been pruned. A
    /// cursor older than this may have missed a delete that no longer exists.
    pub pruned_rev: i64,
}

impl SyncClock {
    /// Whether rows newer than `since` are enough to bring `user_id`'s client
    /// holding `since` up to date. Anything else gets the full snapshot.
    pub fn can_serve_delta(&self, since: &Cursor, user_id: i64) -> bool {
        since.epoch == self.user_epoch(user_id)
            // A cursor from the future means this database's sequence went
            // backwards (a restore without an epoch rotation).
            && since.rev < self.rev
            && since.rev >= self.pruned_rev
    }

    /// The cursor to hand `user_id` at the end of this transaction.
    pub fn cursor(&self, user_id: i64) -> Cursor {
        Cursor {
            epoch: self.user_epoch(user_id),
            rev: self.rev,
        }
    }

    fn user_epoch(&self, user_id: i64) -> String {
        format!("{}.{user_id}", self.epoch)
    }
}

/// Creates the clock's single row with a fresh random epoch. No-op once it
/// exists.
pub(super) async fn seed(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO sync_meta (id, epoch, rev, pruned_rev) VALUES (1, ?1, 0, 0)",
    )
    .bind(new_epoch())
    .execute(pool)
    .await
    .context("seed sync_meta")?;
    Ok(())
}

fn new_epoch() -> String {
    use rand::Rng;
    let bytes: [u8; 8] = rand::thread_rng().r#gen();
    super::hex(&bytes)
}

/// Takes the next revision. Call it first in the sync transaction: as a write,
/// it takes SQLite's write lock up front, so the transaction can't fail later
/// on a read-to-write upgrade while another sync holds the lock.
pub async fn next_rev_tx(conn: &mut sqlx::SqliteConnection) -> anyhow::Result<SyncClock> {
    let row = sqlx::query(
        "UPDATE sync_meta SET rev = rev + 1 WHERE id = 1
         RETURNING epoch, rev, pruned_rev",
    )
    .fetch_one(&mut *conn)
    .await
    .context("take next sync rev")?;
    Ok(SyncClock {
        epoch: row.get("epoch"),
        rev: row.get("rev"),
        pruned_rev: row.get("pruned_rev"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::fresh_db;

    fn clock(rev: i64, pruned_rev: i64) -> SyncClock {
        SyncClock {
            epoch: "e".into(),
            rev,
            pruned_rev,
        }
    }

    fn at(epoch: &str, rev: i64) -> Cursor {
        Cursor {
            epoch: epoch.into(),
            rev,
        }
    }

    #[test]
    fn delta_needs_same_epoch_and_user_a_past_cursor_and_no_pruned_gap() {
        assert!(clock(10, 0).can_serve_delta(&at("e.1", 9), 1));
        assert!(clock(10, 5).can_serve_delta(&at("e.1", 5), 1));
        assert!(
            !clock(10, 0).can_serve_delta(&at("other.1", 9), 1),
            "foreign epoch"
        );
        assert!(
            !clock(10, 0).can_serve_delta(&at("e.2", 9), 1),
            "another user's cursor"
        );
        assert!(
            !clock(10, 0).can_serve_delta(&at("e", 9), 1),
            "no user part"
        );
        assert!(
            !clock(10, 0).can_serve_delta(&at("e.1", 10), 1),
            "cursor not behind"
        );
        assert!(
            !clock(10, 0).can_serve_delta(&at("e.1", 42), 1),
            "cursor ahead"
        );
        assert!(
            !clock(10, 6).can_serve_delta(&at("e.1", 5), 1),
            "pruned past cursor"
        );
        assert_eq!(clock(10, 0).cursor(7), at("e.7", 10));
    }

    #[tokio::test]
    async fn revisions_increase_and_keep_the_epoch() {
        let db = fresh_db().await;
        let mut conn = db.acquire().await.unwrap();
        let a = next_rev_tx(&mut conn).await.unwrap();
        let b = next_rev_tx(&mut conn).await.unwrap();
        assert_eq!(b.rev, a.rev + 1);
        assert_eq!(a.epoch, b.epoch);
        assert_eq!(a.epoch.len(), 16);
    }

    #[tokio::test]
    async fn seeding_twice_keeps_the_epoch_and_rev() {
        let db = fresh_db().await;
        let mut conn = db.acquire().await.unwrap();
        let before = next_rev_tx(&mut conn).await.unwrap();
        drop(conn);
        seed(&db).await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        let after_seed = next_rev_tx(&mut conn).await.unwrap();
        assert_eq!(before.epoch, after_seed.epoch);
        assert_eq!(after_seed.rev, before.rev + 1, "seeding must not reset rev");
    }
}
