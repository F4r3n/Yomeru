//! The `settings` table: each user's synced scheduler knobs, one row per
//! email, merged last-write-wins on `updated_ms`.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::Row;

/// A user's synced scheduler settings. One row per email. `updated_ms` is the
/// last-write-wins merge key (wall-clock ms of the client edit that produced
/// these values). Device-local fields (server URL/email/token) are never
/// stored here — only the knobs that affect scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub graduation_interval_days: i64,
    pub interval_scale: f64,
    pub max_session_cards: i64,
    /// FSRS desired retention. Defaulted so a client that predates this field
    /// can still sync its other settings without failing the whole request.
    #[serde(default = "default_request_retention")]
    pub request_retention: f64,
    pub updated_ms: f64,
}

fn default_request_retention() -> f64 {
    0.9
}

/// Upserts a user's scheduler settings, last-write-wins: the incoming row
/// replaces the stored one only if its `updated_ms` is greater than or equal
/// to what's stored (ties favor the incoming write, matching the cards merge).
pub async fn upsert_settings<'e, E>(ex: E, email: &str, s: &Settings) -> anyhow::Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO settings
             (email, graduation_interval_days, interval_scale, max_session_cards,
              request_retention, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(email) DO UPDATE SET
             graduation_interval_days = excluded.graduation_interval_days,
             interval_scale = excluded.interval_scale,
             max_session_cards = excluded.max_session_cards,
             request_retention = excluded.request_retention,
             updated_ms = excluded.updated_ms
         WHERE excluded.updated_ms >= settings.updated_ms",
    )
    .bind(email)
    .bind(s.graduation_interval_days)
    .bind(s.interval_scale)
    .bind(s.max_session_cards)
    .bind(s.request_retention)
    .bind(s.updated_ms)
    .execute(ex)
    .await
    .context("upsert settings")?;
    Ok(())
}

/// Returns the user's stored settings, or `Ok(None)` if they've never synced
/// any (so the client keeps its local defaults).
pub async fn get_settings<'e, E>(ex: E, email: &str) -> anyhow::Result<Option<Settings>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row = sqlx::query(
        "SELECT graduation_interval_days, interval_scale, max_session_cards,
                request_retention, updated_ms
         FROM settings WHERE email = ?1",
    )
    .bind(email)
    .fetch_optional(ex)
    .await
    .context("query get_settings")?;
    Ok(row.map(|r| Settings {
        graduation_interval_days: r.get("graduation_interval_days"),
        interval_scale: r.get("interval_scale"),
        max_session_cards: r.get("max_session_cards"),
        request_retention: r.get("request_retention"),
        updated_ms: r.get("updated_ms"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::*;

    #[tokio::test]
    async fn settings_insert_then_read_roundtrip() {
        let db = fresh_db().await;
        assert!(get_settings(&db, ALICE).await.unwrap().is_none());
        upsert_settings(&db, ALICE, &settings(100.0, 0.85))
            .await
            .unwrap();
        let got = get_settings(&db, ALICE).await.unwrap().unwrap();
        assert_eq!(got.request_retention, 0.85);
        assert_eq!(got.updated_ms, 100.0);
    }

    #[tokio::test]
    async fn settings_keep_newer_updated_ms() {
        let db = fresh_db().await;
        upsert_settings(&db, ALICE, &settings(200.0, 0.90))
            .await
            .unwrap();
        // Stale write must lose.
        upsert_settings(&db, ALICE, &settings(100.0, 0.70))
            .await
            .unwrap();
        let got = get_settings(&db, ALICE).await.unwrap().unwrap();
        assert_eq!(got.updated_ms, 200.0);
        assert_eq!(got.request_retention, 0.90);
    }

    #[tokio::test]
    async fn settings_isolated_per_user() {
        let db = fresh_db().await;
        upsert_settings(&db, ALICE, &settings(100.0, 0.80))
            .await
            .unwrap();
        upsert_settings(&db, BOB, &settings(100.0, 0.95))
            .await
            .unwrap();
        assert_eq!(
            get_settings(&db, ALICE)
                .await
                .unwrap()
                .unwrap()
                .request_retention,
            0.80
        );
        assert_eq!(
            get_settings(&db, BOB)
                .await
                .unwrap()
                .unwrap()
                .request_retention,
            0.95
        );
    }
}
