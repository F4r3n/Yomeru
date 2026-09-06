//! The `/api/sync` endpoint: the whole card/settings reconciliation in one
//! request.
//!
//! The merge rules themselves live in [`crate::db::cards`] and
//! [`crate::db::deletions`]; this module is the transport and the transaction
//! boundary.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use super::{db_err, extract_bearer, now_ms};
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
}

pub async fn sync_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SyncBody>,
) -> Result<Response, Response> {
    if state
        .limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }

    // Auth is required even in dev mode — dev just skips OTP+SMTP so the
    // token is auto-issued by /api/auth/request. Every sync still needs a
    // valid session so we know whose cards to read/write.
    let token = match extract_bearer(&headers) {
        Some(t) => t.to_string(),
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "missing token" })),
            )
                .into_response());
        }
    };

    let now = now_ms();
    let email = match db::validate_session(&state.db, &token, now)
        .await
        .map_err(|e| db_err("validate_session", e))?
    {
        Some(e) => e,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid or expired session" })),
            )
                .into_response());
        }
    };

    // One transaction for the whole round trip. The reads have to see exactly
    // the state the writes just produced: run separately, two devices syncing
    // at once can interleave and each be handed a merged snapshot that never
    // existed, which is how a client ends up adopting a half-applied merge.
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_err("begin sync tx", e.into()))?;

    let incoming: Vec<db::Deletion> = body.deletions.iter().map(|d| d.to_deletion(now)).collect();
    db::apply_deletions_tx(&mut tx, &email, &incoming)
        .await
        .map_err(|e| db_err("apply_deletions", e))?;

    db::upsert_cards_tx(&mut tx, &email, &body.cards, now)
        .await
        .map_err(|e| db_err("upsert_cards", e))?;

    if let Some(ref s) = body.settings {
        db::upsert_settings(&mut *tx, &email, s)
            .await
            .map_err(|e| db_err("upsert_settings", e))?;
    }

    let merged = db::get_all_cards(&mut *tx, &email)
        .await
        .map_err(|e| db_err("get_all_cards", e))?;

    let tombstones = db::get_all_deletions(&mut *tx, &email)
        .await
        .map_err(|e| db_err("get_all_deletions", e))?;

    let settings = db::get_settings(&mut *tx, &email)
        .await
        .map_err(|e| db_err("get_settings", e))?;

    tx.commit()
        .await
        .map_err(|e| db_err("commit sync tx", e.into()))?;

    Ok(Json(SyncResponse {
        cards: merged,
        deletions: tombstones,
        settings,
    })
    .into_response())
}

// ---- Lookup endpoints (no auth, rate-limited by lookup_limiter) ----------
