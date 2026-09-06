//! The `cards` table: the server's mirror of each user's deck.
//!
//! The merge rule lives in [`upsert_cards`], and is one half of a pair — see
//! [`super::deletions::apply_deletions`] for the other. Both compare a card's
//! `added_ms` against a tombstone's `deleted_at` to tell a genuine re-add from
//! a stale replica, and they have to stay symmetric: if only one guards, a
//! delete and a re-add racing each other lose data in one direction or the other.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;

#[cfg(test)]
use super::Db;

/// A spaced-repetition card as exchanged with clients and stored one field per
/// column. The server owns this shape now: adding a field on the client means
/// adding a column to `CARDS_DDL` and bumping the sentinel in
/// `drop_stale_cards`, or the field is dropped on round-trip.
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
    /// Wall-clock ms of the last write to this card on any device, and the sync
    /// merge key. Distinct from `last_review_ms`, which only moves on a review
    /// and is reset to NULL by `reset_progression` — a status promotion or a
    /// priority edit advances this and not that, which is exactly why the merge
    /// can't key on the review time. Defaulted so a client that predates the
    /// field still syncs; [`normalize_version`] derives a usable value for it.
    #[serde(default)]
    pub updated_ms: f64,
}

/// The effective merge version for an incoming card. A client that predates
/// `updated_ms` sends 0; deriving from the review/added times (both already
/// agreed on by every device) keeps such a card comparable instead of losing
/// every merge, and yields the same answer on whichever device it arrives from.
///
/// Clamped to `now` for the same reason tombstone times are: the value is
/// client wall-clock, so a device whose clock runs a month fast would otherwise
/// stamp every card it touches a month ahead, win every merge for that month,
/// and revert every other device's edits on each sync.
fn normalize_version(c: &Card, now: i64) -> f64 {
    let raw = if c.updated_ms > 0.0 {
        c.updated_ms
    } else {
        c.last_review_ms.unwrap_or(0.0).max(c.added_ms)
    };
    #[allow(clippy::cast_precision_loss)]
    let now_f = now as f64;
    if raw > now_f { now_f } else { raw }
}

/// Upserts incoming cards for `email`: replaces a stored card only if the
/// incoming one is newer (higher `updated_ms`).
///
/// A card whose id carries a tombstone is only accepted when it was added
/// *after* the delete (`added_ms > deleted_at`), i.e. a genuine re-add — and
/// only then is the tombstone cleared. This guard is what stops a delete from
/// being undone: every client uploads its whole local card set on every sync,
/// so a device that hasn't pulled the delete yet re-sends its stale copy, and
/// without the check that copy would both resurrect the card and destroy the
/// tombstone, leaving no device able to learn about the delete.
///
/// [`super::deletions::apply_deletions`] carries the mirror of this guard, and
/// the two only work as a pair — see the module docs.
///
/// This is why import stamps a fresh `added_ms` on the cards it restores: a
/// card carrying its original `added_ms` reads as a stale replica here, gets
/// refused, and is then deleted locally on the next sync, so a restored backup
/// would appear and silently vanish.
///
/// Test-only: production always calls [`upsert_cards_tx`] inside the sync
/// handler's transaction. Kept because almost every test here is about the
/// merge rule rather than transaction plumbing, and a standalone call keeps
/// them readable.
#[cfg(test)]
pub async fn upsert_cards(db: &Db, email: &str, cards: &[Card], now: i64) -> anyhow::Result<()> {
    let mut tx = db.begin().await.context("begin upsert tx")?;
    upsert_cards_tx(&mut tx, email, cards, now).await?;
    tx.commit().await.context("commit upsert tx")?;
    Ok(())
}

/// [`upsert_cards`] without the surrounding transaction, so a caller that has
/// one open — the sync handler, which needs its writes and reads to land as a
/// single snapshot — can enlist this in it.
pub async fn upsert_cards_tx(
    conn: &mut sqlx::SqliteConnection,
    email: &str,
    cards: &[Card],
    now: i64,
) -> anyhow::Result<()> {
    // One read for the whole batch rather than one per card. Clients upload
    // their entire deck on every sync, so a per-card lookup made the cost of a
    // sync scale with deck size for the sake of a table that is usually tiny.
    let tombstones: HashMap<String, i64> =
        sqlx::query("SELECT id, deleted_at FROM deletions WHERE email = ?1")
            .bind(email)
            .fetch_all(&mut *conn)
            .await
            .context("read tombstones for upsert")?
            .iter()
            .map(|r| (r.get("id"), r.get("deleted_at")))
            .collect();

    for c in cards {
        if c.id.is_empty() {
            continue;
        }
        // Resolved before the insert rather than folded into its WHERE: the
        // insert guard and the tombstone clear have to agree on whether this
        // card won, and two separate predicates can drift apart.
        let deleted_at = tombstones.get(&c.id).copied();
        if deleted_at.is_some_and(|d| d as f64 >= c.added_ms) {
            continue; // stale replica of a deleted card, not a re-add
        }

        sqlx::query(
            "INSERT INTO cards
                 (email, id, sequence, direction, due_ms, stability, difficulty,
                  reps, lapses, state, last_review_ms, added_ms, status, priority,
                  updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
                 priority = excluded.priority,
                 updated_ms = excluded.updated_ms
             WHERE excluded.updated_ms >= cards.updated_ms",
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
        .bind(normalize_version(c, now))
        .execute(&mut *conn)
        .await
        .context("upsert card")?;

        if deleted_at.is_some() {
            sqlx::query("DELETE FROM deletions WHERE email = ?1 AND id = ?2")
                .bind(email)
                .bind(&c.id)
                .execute(&mut *conn)
                .await
                .context("clear matching tombstone")?;
        }
    }
    Ok(())
}

pub async fn get_all_cards<'e, E>(ex: E, email: &str) -> anyhow::Result<Vec<Card>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query(
        "SELECT id, sequence, direction, due_ms, stability, difficulty,
                reps, lapses, state, last_review_ms, added_ms, status, priority,
                updated_ms
         FROM cards WHERE email = ?1",
    )
    .bind(email)
    .fetch_all(ex)
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
            updated_ms: r.get("updated_ms"),
        })
        .collect();
    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::deletions::{apply_deletions, get_all_deletions};
    use crate::db::test_support::*;

    #[tokio::test]
    async fn upsert_inserts_new_cards() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, "a::recognition");
    }

    #[tokio::test]
    async fn upsert_keeps_newer_last_review() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        // Older incoming write should be ignored.
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(50.0))], NOW)
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].last_review_ms, Some(100.0));
    }

    #[tokio::test]
    async fn upsert_replaces_with_equal_or_newer_last_review() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        let mut newer = card("a::recognition", Some(200.0));
        newer.sequence = 1_586_270;
        upsert_cards(&db, ALICE, &[newer], NOW).await.unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].sequence, 1_586_270);
        assert_eq!(stored[0].last_review_ms, Some(200.0));
    }

    #[tokio::test]
    async fn genuine_re_add_clears_matching_tombstone() {
        // Re-add must win over an old delete: otherwise a client that brings
        // back a card after deleting it would see the resurrection wiped
        // out on the next sync. "Genuine" means added after the delete —
        // re-adding through the UI mints a fresh card with `added_ms = now`.
        let db = fresh_db().await;
        apply_deletions(&db, ALICE, &[del("a::recognition", 1_000)])
            .await
            .unwrap();
        let mut re_added = card("a::recognition", None);
        re_added.added_ms = 2_000.0;
        upsert_cards(&db, ALICE, &[re_added], NOW).await.unwrap();
        assert_eq!(get_all_cards(&db, ALICE).await.unwrap().len(), 1);
        assert!(get_all_deletions(&db, ALICE).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stale_replica_cannot_resurrect_a_deleted_card() {
        // The production bug. Every client uploads its whole card set on every
        // sync, so a device that hasn't pulled the delete yet re-sends its old
        // copy. That copy used to both resurrect the card and clear the
        // tombstone, after which no device could ever learn about the delete.
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        apply_deletions(&db, ALICE, &[del("a::recognition", 5_000)])
            .await
            .unwrap();

        // Same card as before the delete: added_ms predates the tombstone.
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();

        assert!(
            get_all_cards(&db, ALICE).await.unwrap().is_empty(),
            "stale copy must not resurrect the card"
        );
        assert_eq!(
            deleted_ids(&db, ALICE).await,
            ["a::recognition"],
            "tombstone must survive so the lagging device still learns the delete"
        );
    }

    #[tokio::test]
    async fn promotion_survives_a_stale_copy_carrying_a_later_review() {
        // Staging→Active never touches `last_review_ms`, so under the old
        // merge key a stale copy with any review time reverted the promotion —
        // which is what made devices disagree on their Active card counts.
        let db = fresh_db().await;
        let mut promoted = card("a::recognition", None);
        promoted.status = "active".to_string();
        promoted.updated_ms = 5_000.0;
        upsert_cards(&db, ALICE, &[promoted], NOW).await.unwrap();

        let mut stale = card("a::recognition", Some(9_000.0));
        stale.status = "staging".to_string();
        stale.updated_ms = 3_000.0;
        upsert_cards(&db, ALICE, &[stale], NOW).await.unwrap();

        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].status, "active");
    }

    #[tokio::test]
    async fn reset_survives_a_stale_reviewed_copy() {
        // `reset_progression` clears `reps` and sets `last_review_ms` back to
        // NULL, moving the old merge key *backwards* — so a reset always lost
        // to the pre-reset copy on another device and silently undid itself.
        let db = fresh_db().await;
        let mut reviewed = card("a::recognition", Some(9_000.0));
        reviewed.reps = 12;
        reviewed.updated_ms = 9_000.0;
        upsert_cards(&db, ALICE, &[reviewed.clone()], NOW)
            .await
            .unwrap();

        let mut reset = card("a::recognition", None);
        reset.reps = 0;
        reset.updated_ms = 10_000.0;
        upsert_cards(&db, ALICE, &[reset], NOW).await.unwrap();

        // The stale pre-reset copy arrives afterwards and must lose.
        upsert_cards(&db, ALICE, &[reviewed], NOW).await.unwrap();

        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(
            stored[0].reps, 0,
            "reset must not be undone by a stale copy"
        );
        assert_eq!(stored[0].last_review_ms, None);
    }

    #[tokio::test]
    async fn legacy_card_without_updated_ms_is_ordered_by_review_time() {
        // A client that predates `updated_ms` sends 0. Falling back to the
        // review/added times keeps it comparable instead of losing every merge.
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(8_000.0))], NOW)
            .await
            .unwrap();
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(2_000.0))], NOW)
            .await
            .unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].last_review_ms, Some(8_000.0));
        assert_eq!(stored[0].updated_ms, 8_000.0);
    }

    #[tokio::test]
    async fn fast_clock_card_version_is_clamped_to_now() {
        // A device a month ahead would otherwise win every merge for a month
        // and revert every other device's edits on each sync.
        let db = fresh_db().await;
        let mut skewed = card("a::recognition", None);
        skewed.updated_ms = (NOW + 30 * 86_400_000) as f64;
        skewed.status = "staging".to_string();
        upsert_cards(&db, ALICE, &[skewed], NOW).await.unwrap();

        let mut honest = card("a::recognition", None);
        honest.updated_ms = NOW as f64;
        honest.status = "active".to_string();
        upsert_cards(&db, ALICE, &[honest], NOW).await.unwrap();

        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(
            stored[0].status, "active",
            "an honest write at server-now must still be able to win"
        );
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
        upsert_cards(&db, ALICE, &[reviewed], NOW).await.unwrap();

        let mut stale = card("猫::recognition", None);
        stale.due_ms = 1_700_000_000_000.0; // older, "due now" copy
        upsert_cards(&db, ALICE, &[stale], NOW).await.unwrap();

        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].last_review_ms, Some(1_779_000_000_000.0));
        assert_eq!(
            stored[0].due_ms, 1_780_000_000_000.0,
            "reviewed schedule must survive a stale unreviewed push"
        );
    }

    #[tokio::test]
    async fn upsert_and_read_roundtrips_priority() {
        let db = fresh_db().await;
        let mut c = card("a::recognition", Some(100.0));
        c.priority = 5;
        upsert_cards(&db, ALICE, &[c], NOW).await.unwrap();
        let stored = get_all_cards(&db, ALICE).await.unwrap();
        assert_eq!(stored[0].priority, 5);
    }

    #[tokio::test]
    async fn users_cannot_see_each_others_cards() {
        let db = fresh_db().await;
        upsert_cards(&db, ALICE, &[card("a::recognition", Some(100.0))], NOW)
            .await
            .unwrap();
        upsert_cards(&db, BOB, &[card("b::recognition", Some(200.0))], NOW)
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
        upsert_cards(&db, ALICE, &[alice_card], NOW).await.unwrap();
        upsert_cards(&db, BOB, &[bob_card], NOW).await.unwrap();
        assert_eq!(
            get_all_cards(&db, ALICE).await.unwrap()[0].sequence,
            1_467_640
        );
        assert_eq!(
            get_all_cards(&db, BOB).await.unwrap()[0].sequence,
            1_586_270
        );
    }
}
