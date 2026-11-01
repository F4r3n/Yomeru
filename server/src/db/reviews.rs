//! The `reviews` table: an append-only log of every answer a user gave, real
//! reviews and drills alike.
//!
//! Clients upload the rows they've logged since their last successful sync and
//! drop them locally once acknowledged; nothing is ever downloaded back. The
//! log is the server's to aggregate — today only into per-card drill counts,
//! which decide the order drill sessions rotate through trouble cards.
//!
//! Rows are never updated, so there is no merge rule: a row is identified by
//! `(card_id, reviewed_ms)` and a re-sent one is simply ignored.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;

#[cfg(test)]
use super::Db;

/// One logged answer, as clients put it on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewLog {
    pub card_id: String,
    /// The button pressed: 1 = Again … 4 = Easy.
    pub rating: u8,
    /// `"review"` for a scheduled review, `"drill"` for a drill answer that
    /// left the schedule untouched.
    pub kind: String,
    /// Client wall-clock ms of the answer. `f64` like every other timestamp in
    /// this API, since that is what JS `Date.now()` serializes as.
    pub reviewed_ms: f64,
}

/// Wire `kind` → stored code. `None` for an unknown kind.
fn kind_code(kind: &str) -> Option<i64> {
    match kind {
        "review" => Some(0),
        "drill" => Some(1),
        _ => None,
    }
}

const DRILL: i64 = 1;

/// Whether a row is worth storing. A malformed row is dropped rather than
/// failing the whole sync: the cards and tombstones in the same request still
/// matter, and a client bug in the log shouldn't wedge them.
fn is_valid(r: &ReviewLog) -> bool {
    !r.card_id.is_empty()
        && (1..=4).contains(&r.rating)
        && kind_code(&r.kind).is_some()
        && r.reviewed_ms.is_finite()
        && r.reviewed_ms >= 0.0
}

/// Appends `rows` to the user's log inside the caller's transaction. A row
/// already stored (same card, same ms — a retried upload) is ignored.
///
/// Times are stored exactly as sent, never clamped: they are part of the row's
/// identity, so rewriting one would make a retried upload look new. Nothing is
/// ordered against them either, unlike tombstones. A future time — a device
/// with a wrong date — is kept too: the client deletes whatever the server
/// acknowledges, so rejecting the row would lose the answer on both sides.
pub async fn insert_reviews_tx(
    conn: &mut sqlx::SqliteConnection,
    user_id: i64,
    rows: &[ReviewLog],
) -> anyhow::Result<()> {
    for r in rows.iter().filter(|r| is_valid(r)) {
        // Whole ms, as every client produces (`Date.now()`). The cast saturates
        // rather than overflowing on a nonsense value.
        #[allow(clippy::cast_possible_truncation)]
        let reviewed_ms = r.reviewed_ms as i64;
        sqlx::query(
            "INSERT OR IGNORE INTO reviews (user_id, kind, card_id, reviewed_ms, rating)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(user_id)
        .bind(kind_code(&r.kind))
        .bind(&r.card_id)
        .bind(reviewed_ms)
        .bind(i64::from(r.rating))
        .execute(&mut *conn)
        .await
        .context("insert review log row")?;
    }
    Ok(())
}

/// Number of drill answers logged per card id. Cards never drilled are absent.
pub async fn get_drill_counts<'e, E>(ex: E, user_id: i64) -> anyhow::Result<HashMap<String, u32>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query(
        "SELECT card_id, COUNT(*) AS n FROM reviews
         WHERE user_id = ?1 AND kind = ?2
         GROUP BY card_id",
    )
    .bind(user_id)
    .bind(DRILL)
    .fetch_all(ex)
    .await
    .context("count drills")?;
    Ok(rows
        .iter()
        .map(|r| {
            let n: i64 = r.get("n");
            (r.get("card_id"), u32::try_from(n).unwrap_or(u32::MAX))
        })
        .collect())
}

/// [`insert_reviews_tx`] in its own transaction. Test-only, like
/// `upsert_cards`: production always enlists in the sync handler's.
#[cfg(test)]
pub async fn insert_reviews(db: &Db, user_id: i64, rows: &[ReviewLog]) -> anyhow::Result<()> {
    let mut tx = db.begin().await.context("begin reviews tx")?;
    insert_reviews_tx(&mut tx, user_id, rows).await?;
    tx.commit().await.context("commit reviews tx")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::auth::get_or_create_user;
    use crate::db::test_support::*;

    async fn user(db: &Db, email: &str) -> i64 {
        let mut conn = db.acquire().await.unwrap();
        get_or_create_user(&mut conn, email).await.unwrap()
    }

    fn row(card_id: &str, kind: &str, rating: u8, at: f64) -> ReviewLog {
        ReviewLog {
            card_id: card_id.to_string(),
            rating,
            kind: kind.to_string(),
            reviewed_ms: at,
        }
    }

    #[tokio::test]
    async fn counts_drills_per_card_and_ignores_reviews() {
        let db = fresh_db().await;
        let alice = user(&db, ALICE).await;
        let rows = [
            row("1::recall", "drill", 1, 10.0),
            row("1::recall", "drill", 3, 20.0),
            row("1::recognition", "drill", 3, 30.0),
            row("1::recall", "review", 3, 40.0),
        ];
        insert_reviews(&db, alice, &rows).await.unwrap();
        let counts = get_drill_counts(&db, alice).await.unwrap();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts["1::recall"], 2);
        assert_eq!(counts["1::recognition"], 1);
    }

    #[tokio::test]
    async fn a_resent_row_is_stored_once() {
        // Clients re-send their outbox until a sync succeeds, so an upload whose
        // response was lost arrives twice.
        let db = fresh_db().await;
        let alice = user(&db, ALICE).await;
        let rows = [row("1::recall", "drill", 3, 10.0)];
        insert_reviews(&db, alice, &rows).await.unwrap();
        insert_reviews(&db, alice, &rows).await.unwrap();
        assert_eq!(get_drill_counts(&db, alice).await.unwrap()["1::recall"], 1);
    }

    #[tokio::test]
    async fn a_future_dated_row_is_kept_and_deduplicated_on_resend() {
        // The time is part of the row's key. Clamping it to server time would
        // store each resend under a different `now`, counting it again; and
        // dropping it would lose it, since the client deletes what we ack.
        let db = fresh_db().await;
        let alice = user(&db, ALICE).await;
        let rows = [row(
            "1::recall",
            "drill",
            3,
            NOW as f64 + 30.0 * 86_400_000.0,
        )];
        insert_reviews(&db, alice, &rows).await.unwrap();
        insert_reviews(&db, alice, &rows).await.unwrap();
        assert_eq!(get_drill_counts(&db, alice).await.unwrap()["1::recall"], 1);
    }

    #[tokio::test]
    async fn malformed_rows_are_dropped() {
        let db = fresh_db().await;
        let alice = user(&db, ALICE).await;
        let rows = [
            row("1::recall", "drill", 0, 10.0),
            row("1::recall", "drill", 5, 11.0),
            row("1::recall", "cram", 3, 12.0),
            row("", "drill", 3, 13.0),
            row("1::recall", "drill", 3, f64::NAN),
            row("1::recall", "drill", 3, -1.0),
        ];
        insert_reviews(&db, alice, &rows).await.unwrap();
        assert!(get_drill_counts(&db, alice).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn counts_are_per_user() {
        let db = fresh_db().await;
        let alice = user(&db, ALICE).await;
        insert_reviews(&db, alice, &[row("1::recall", "drill", 3, 10.0)])
            .await
            .unwrap();
        assert!(
            get_drill_counts(&db, user(&db, BOB).await)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
