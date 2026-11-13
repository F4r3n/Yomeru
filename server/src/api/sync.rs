//! The `/api/sync` endpoint: the whole card/settings reconciliation in one
//! request.
//!
//! The merge rules themselves live in [`crate::db::cards`] and
//! [`crate::db::deletions`]; this module is the transport and the transaction
//! boundary.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::HeaderMap,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::{ApiError, db_err, extract_bearer, now_ms, rate_limit};
use crate::AppState;
use crate::db;

#[derive(Deserialize)]
pub struct SyncBody {
    pub cards: Vec<db::Card>,
    /// Deleted card ids, each either a bare string (older clients) or an object
    /// carrying the client's own delete time. The time matters because a device
    /// that deleted while offline for a week would otherwise have the tombstone
    /// stamped with server receipt time, beating a re-add another device made
    /// yesterday.
    #[serde(default)]
    pub deletions: Vec<db::DeletionEntry>,
    /// Client's current scheduler settings. Optional so older clients that
    /// don't send settings keep working (cards-only sync).
    #[serde(default)]
    pub settings: Option<db::Settings>,
    /// Answers logged since the client's last successful sync. Optional so
    /// older clients, which keep no log, still sync.
    #[serde(default)]
    pub reviews: Vec<db::ReviewLog>,
    /// The cursor from the client's last successful sync. When present and
    /// still usable, the response is a delta from it. Absent from clients
    /// that predate delta sync, and from any client on its first sync.
    #[serde(default)]
    pub since: Option<db::Cursor>,
}

#[derive(Serialize)]
pub struct SyncResponse {
    pub cards: Vec<db::Card>,
    /// Tombstone ids only. Clients don't need the delete times back: a re-add
    /// that genuinely superseded a delete has already cleared the tombstone
    /// server-side, so anything still listed here is a delete that stands.
    pub deletions: Vec<String>,
    /// The user's stored settings after the merge, or `None` if they've never
    /// synced any. Omitted from the JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<db::Settings>,
    /// Drill answers per card id across the user's whole log, after this
    /// request's rows were added. Older clients ignore it.
    pub drill_counts: HashMap<String, u32>,
    /// Hand this back as `since` next time.
    pub cursor: db::Cursor,
    /// `true`: `cards`, `deletions` and `drill_counts` are the user's complete
    /// state, as before delta sync. A client that sent `since` and gets `true`
    /// must re-sync with its whole deck: the server couldn't vouch for what it
    /// already holds, and may have lost rows only that client still has. `false`: they hold only what changed after
    /// the client's cursor plus the server's copy of every id it sent, and
    /// the client merges `drill_counts` into its cache instead of replacing it.
    pub full: bool,
}

pub async fn sync_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SyncBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.sync_limiter, addr, &headers)?;

    // Auth is required even in dev mode — dev just skips OTP+SMTP so the
    // token is auto-issued by /api/auth/request. Every sync still needs a
    // valid session so we know whose cards to read/write.
    let token = extract_bearer(&headers)
        .ok_or(ApiError::Unauthorized(Some("missing token")))?
        .to_string();

    let now = now_ms();
    let db::SessionUser { id: user_id, email } = db::validate_session(&state.db, &token, now)
        .await
        .map_err(|e| db_err("validate_session", e))?
        .ok_or(ApiError::Unauthorized(Some("invalid or expired session")))?;

    // One transaction for the whole round trip. The reads have to see exactly
    // the state the writes just produced: run separately, two devices syncing
    // at once can interleave and each be handed a merged snapshot that never
    // existed, which is how a client ends up adopting a half-applied merge.
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_err("begin sync tx", e.into()))?;

    // First, so the transaction holds the write lock from its first statement.
    let clock = db::next_rev_tx(&mut tx)
        .await
        .map_err(|e| db_err("next_rev", e))?;

    let incoming: Vec<db::Deletion> = body.deletions.iter().map(|d| d.to_deletion(now)).collect();
    db::apply_deletions_tx(&mut tx, &email, &incoming, clock.rev)
        .await
        .map_err(|e| db_err("apply_deletions", e))?;

    db::upsert_cards_tx(&mut tx, &email, &body.cards, now, clock.rev)
        .await
        .map_err(|e| db_err("upsert_cards", e))?;

    db::insert_reviews_tx(&mut tx, user_id, &body.reviews, clock.rev)
        .await
        .map_err(|e| db_err("insert_reviews", e))?;

    if let Some(ref s) = body.settings {
        db::upsert_settings(&mut *tx, &email, s)
            .await
            .map_err(|e| db_err("upsert_settings", e))?;
    }

    let delta_from = body
        .since
        .as_ref()
        .filter(|c| clock.can_serve_delta(c, user_id))
        .map(|c| c.rev);

    let (cards, deletions, drill_counts) = if let Some(since) = delta_from {
        // Every id the client sent, card or tombstone: whichever the server
        // now holds for it goes back, so the client learns how its upload was
        // ruled on even when the ruling kept an older row. See
        // `db::get_cards_since`.
        let sent: Vec<&str> = body
            .cards
            .iter()
            .map(|c| c.id.as_str())
            .chain(incoming.iter().map(|d| d.id.as_str()))
            .collect();
        let reviewed: Vec<&str> = body.reviews.iter().map(|r| r.card_id.as_str()).collect();
        (
            db::get_cards_since(&mut *tx, &email, since, &sent)
                .await
                .map_err(|e| db_err("get_cards_since", e))?,
            db::get_deletions_since(&mut *tx, &email, since, &sent)
                .await
                .map_err(|e| db_err("get_deletions_since", e))?,
            db::get_drill_counts_since(&mut *tx, user_id, since, &reviewed)
                .await
                .map_err(|e| db_err("get_drill_counts_since", e))?,
        )
    } else {
        (
            db::get_all_cards(&mut *tx, &email)
                .await
                .map_err(|e| db_err("get_all_cards", e))?,
            db::get_all_deletions(&mut *tx, &email)
                .await
                .map_err(|e| db_err("get_all_deletions", e))?,
            db::get_drill_counts(&mut *tx, user_id)
                .await
                .map_err(|e| db_err("get_drill_counts", e))?,
        )
    };

    let settings = db::get_settings(&mut *tx, &email)
        .await
        .map_err(|e| db_err("get_settings", e))?;

    tx.commit()
        .await
        .map_err(|e| db_err("commit sync tx", e.into()))?;

    Ok(Json(SyncResponse {
        cards,
        deletions,
        settings,
        drill_counts,
        cursor: clock.cursor(user_id),
        full: delta_from.is_none(),
    }))
}

// ---- Lookup endpoints (no auth, rate-limited by lookup_limiter) ----------
