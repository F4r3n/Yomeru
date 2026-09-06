//! The `deletions` table: tombstones recording that a card was deleted, and
//! when.
//!
//! The time matters because clients re-send pending tombstones until a sync
//! succeeds: a delete made offline a week ago must still be ordered against a
//! re-add another device made yesterday. See [`super::cards::upsert_cards`] for
//! the matching guard on the write path.

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Db;

/// A tombstone: a card id plus when the user deleted it, in client wall-clock
/// ms. The time is what lets [`upsert_cards`] tell a genuine re-add (added
/// after the delete) from a stale replica uploaded by a device that hasn't
/// pulled the delete yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deletion {
    pub id: String,
    pub deleted_at: i64,
}

/// One entry of a client's `deletions` list. Clients that predate the delete
/// time send a bare id string; current ones send the object. Untagged so both
/// shapes live under the same field name — variant order matters, since a bare
/// string can only match [`DeletionEntry::Id`].
///
/// `deleted_at` is `f64` because that is what the client puts on the wire (JS
/// `Date.now()`, serialized as `1757000000123.0`), matching every other
/// timestamp in this API. An `i64` here would reject that number, and because
/// the enum is untagged the failure surfaces as an unhelpful "did not match any
/// variant" over the *whole* request rather than a field-level error.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DeletionEntry {
    Timed { id: String, deleted_at: f64 },
    Id(String),
}

impl DeletionEntry {
    /// Resolves to a tombstone, falling back to `now` for a client that didn't
    /// send a time. `deleted_at` is client wall-clock, so it's clamped to `now`:
    /// a device with a fast clock must not be able to claim a delete far in the
    /// future and outrank every re-add made after it. A negative or NaN value
    /// (a broken client) also lands on `now`.
    pub fn to_deletion(&self, now: i64) -> Deletion {
        match self {
            Self::Timed { id, deleted_at } => Deletion {
                id: id.clone(),
                deleted_at: ms_to_epoch(*deleted_at, now),
            },
            Self::Id(id) => Deletion {
                id: id.clone(),
                deleted_at: now,
            },
        }
    }
}

/// Clamps a client-supplied wall-clock ms value into `0..=now` as an integer.
/// NaN fails both comparisons and falls through to `now`.
fn ms_to_epoch(ms: f64, now: i64) -> i64 {
    #[allow(clippy::cast_precision_loss)]
    let now_f = now as f64;
    if ms >= 0.0 && ms <= now_f {
        #[allow(clippy::cast_possible_truncation)]
        return ms as i64;
    }
    now
}

/// Applies incoming tombstones for `email`: drops each id from `cards` and
/// records the tombstone so the user's other clients can replay the delete.
///
/// A repeat delete keeps the *earliest* `deleted_at`. Every client re-sends its
/// pending tombstones until they're acknowledged, so taking the latest would
/// let a lagging device keep pushing the delete time forward and outrank a
/// re-add that genuinely came after the original delete.
///
/// Test-only, for the same reason as [`super::cards::upsert_cards`].
#[cfg(test)]
pub async fn apply_deletions(db: &Db, email: &str, deletions: &[Deletion]) -> anyhow::Result<()> {
    let mut tx = db.begin().await.context("begin deletions tx")?;
    apply_deletions_tx(&mut tx, email, deletions).await?;
    tx.commit().await.context("commit deletions tx")?;
    Ok(())
}

/// [`apply_deletions`] without the surrounding transaction — see
/// [`super::cards::upsert_cards_tx`].
pub async fn apply_deletions_tx(
    conn: &mut sqlx::SqliteConnection,
    email: &str,
    deletions: &[Deletion],
) -> anyhow::Result<()> {
    if deletions.is_empty() {
        return Ok(());
    }
    for d in deletions {
        if d.id.is_empty() {
            continue;
        }
        // Mirror of the re-add guard in `upsert_cards`, and needed for the same
        // reason from the other direction: a client re-sends pending tombstones
        // until a sync succeeds, so a delete whose response was lost can arrive
        // after another device legitimately re-added the card. Without this the
        // stale delete wins and the re-add is gone everywhere — the deleting
        // device has no copy left for `upsert_cards` to restore.
        let added_ms: Option<f64> =
            sqlx::query_scalar("SELECT added_ms FROM cards WHERE email = ?1 AND id = ?2")
                .bind(email)
                .bind(&d.id)
                .fetch_optional(&mut *conn)
                .await
                .context("read card for deletion guard")?;
        if added_ms.is_some_and(|a| a > d.deleted_at as f64) {
            continue; // the stored card is a re-add that postdates this delete
        }

        sqlx::query("DELETE FROM cards WHERE email = ?1 AND id = ?2")
            .bind(email)
            .bind(&d.id)
            .execute(&mut *conn)
            .await
            .context("delete card")?;
        sqlx::query(
            "INSERT INTO deletions (email, id, deleted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(email, id) DO UPDATE SET
                 deleted_at = MIN(deletions.deleted_at, excluded.deleted_at)",
        )
        .bind(email)
        .bind(&d.id)
        .bind(d.deleted_at)
        .execute(&mut *conn)
        .await
        .context("upsert tombstone")?;
    }
    Ok(())
}

pub async fn get_all_deletions<'e, E>(ex: E, email: &str) -> anyhow::Result<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM deletions WHERE email = ?1")
        .bind(email)
        .fetch_all(ex)
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
    use crate::db::cards::{get_all_cards, upsert_cards};
    use crate::db::test_support::*;

    #[test]
    fn bare_id_deletion_deserializes_and_takes_receipt_time() {
        // The shape a client that predates the delete time sends.
        let parsed: Vec<DeletionEntry> = serde_json::from_str(r#"["a::recognition"]"#).unwrap();
        let d = parsed[0].to_deletion(9_000);
        assert_eq!(d.id, "a::recognition");
        assert_eq!(d.deleted_at, 9_000);
    }

    #[test]
    fn timed_deletion_keeps_the_client_delete_time() {
        // The point of sending the time: a delete made while offline keeps when
        // it happened, so a re-add another device made afterwards still wins.
        // The literal is written the way serde_json renders the client's f64 —
        // a trailing `.0` that an i64 field would reject outright, taking the
        // whole request down with an untagged "matched no variant" error.
        let parsed: Vec<DeletionEntry> =
            serde_json::from_str(r#"[{"id":"a::recognition","deleted_at":1757000000123.0}]"#)
                .unwrap();
        assert_eq!(
            parsed[0].to_deletion(1_757_000_009_999).deleted_at,
            1_757_000_000_123
        );
    }

    #[test]
    fn future_delete_time_is_clamped_to_now() {
        // A device with a fast clock must not claim a delete far in the future
        // and outrank every re-add anyone makes after it.
        let parsed: Vec<DeletionEntry> =
            serde_json::from_str(r#"[{"id":"a::recognition","deleted_at":99000.0}]"#).unwrap();
        assert_eq!(parsed[0].to_deletion(9_000).deleted_at, 9_000);
    }

    #[test]
    fn nonsense_delete_time_falls_back_to_now() {
        let parsed: Vec<DeletionEntry> =
            serde_json::from_str(r#"[{"id":"a","deleted_at":-5.0}]"#).unwrap();
        assert_eq!(parsed[0].to_deletion(9_000).deleted_at, 9_000);
    }

    #[test]
    fn mixed_deletion_shapes_parse_together() {
        // An older client and a current one can be syncing the same account.
        let parsed: Vec<DeletionEntry> =
            serde_json::from_str(r#"["a",{"id":"b","deleted_at":1000.0}]"#).unwrap();
        let out: Vec<Deletion> = parsed.iter().map(|d| d.to_deletion(9_000)).collect();
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].deleted_at, 9_000);
        assert_eq!(out[1].id, "b");
        assert_eq!(out[1].deleted_at, 1_000);
    }

    #[tokio::test]
    async fn apply_deletions_removes_card_and_records_tombstone() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        apply_deletions(&db, ALICE, &[del("a::recognition", 1_700_000_000_000)])
            .await
            .unwrap();
        assert!(get_all_cards(&db, ALICE).await.unwrap().is_empty());
        assert_eq!(deleted_ids(&db, ALICE).await, ["a::recognition"]);
    }

    #[tokio::test]
    async fn a_re_add_survives_a_stale_tombstone_arriving_late() {
        // Device A deletes at T1 but loses the response, so it keeps the
        // tombstone pending. Device B re-adds at T2 > T1 and syncs. When A
        // finally retries, its stale delete must not take B's re-add — A has no
        // copy of the card left, so nothing could restore it.
        let db = fresh_db().await;
        let mut re_added = card("a::recognition", None);
        re_added.added_ms = 2_000.0;
        upsert_cards(&db, ALICE, &[re_added], NOW).await.unwrap();

        apply_deletions(&db, ALICE, &[del("a::recognition", 1_000)])
            .await
            .unwrap();

        assert_eq!(
            get_all_cards(&db, ALICE).await.unwrap().len(),
            1,
            "a delete older than the card must not remove it"
        );
        assert!(
            deleted_ids(&db, ALICE).await.is_empty(),
            "and it must not leave a tombstone that would delete it elsewhere"
        );
    }

    #[tokio::test]
    async fn apply_deletions_is_idempotent() {
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &[del("a::recognition", 100)])
            .await
            .unwrap();
        apply_deletions(&db, ALICE, &[del("a::recognition", 200)])
            .await
            .unwrap();
        assert_eq!(deleted_ids(&db, ALICE).await, ["a::recognition"]);
        assert_eq!(
            deleted_at(&db, ALICE, "a::recognition").await,
            Some(100),
            "a re-sent tombstone must keep the earliest delete time, or a \
             lagging device could keep pushing it past a later re-add"
        );
    }

    #[tokio::test]
    async fn apply_deletions_skips_empty_ids() {
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &[del("", 100), del("a", 100)])
            .await
            .unwrap();
        assert_eq!(deleted_ids(&db, ALICE).await, ["a"]);
    }

    #[tokio::test]
    async fn prune_drops_only_old_tombstones() {
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &[del("old", 100)])
            .await
            .unwrap();
        apply_deletions(&db, ALICE, &[del("recent", 5_000)])
            .await
            .unwrap();
        prune_old_deletions(&db, 1_000).await.unwrap();
        assert_eq!(deleted_ids(&db, ALICE).await, ["recent"]);
    }

    #[tokio::test]
    async fn deletions_isolated_per_user() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("x", Some(100.0))], NOW)
            .await
            .unwrap();
        upsert_cards(&db, BOB, &[card("x", Some(100.0))], NOW)
            .await
            .unwrap();
        // Alice deletes; Bob's copy must survive.
        apply_deletions(&db, ALICE, &[del("x", 1_000)])
            .await
            .unwrap();
        assert!(get_all_cards(&db, ALICE).await.unwrap().is_empty());
        assert_eq!(get_all_cards(&db, BOB).await.unwrap().len(), 1);
        assert_eq!(deleted_ids(&db, ALICE).await, ["x"]);
        assert!(get_all_deletions(&db, BOB).await.unwrap().is_empty());
    }
}
