//! `/api/fsrs/optimize`: fits FSRS weights to the user's review log.
//!
//! Stateless: the weights go back to the client, which saves them in its
//! settings, and settings sync carries them to the other devices.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::HeaderMap,
    response::IntoResponse,
};
use serde::Serialize;
use tokio::sync::Semaphore;

use super::{ApiError, db_err, extract_bearer, now_ms, rate_limit, run_blocking};
use crate::AppState;
use crate::db;
use crate::optimize::{CardHistory, MIN_ITEMS, build_items, optimize};

#[derive(Serialize)]
pub struct OptimizeResponse {
    /// The 19 FSRS-5 weights.
    pub params: Vec<f64>,
    /// Reviews the fit used: those made a day or more after the previous one.
    pub reviews: usize,
}

/// One fit at a time server-wide: each takes seconds of CPU.
static TRAINING: Semaphore = Semaphore::const_new(1);

/// `POST /api/fsrs/optimize`, Bearer token. 400 with a message when the log
/// is too short to beat the defaults, 429 while another fit is running.
pub async fn optimize_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.optimize_limiter, addr, &headers)?;
    let token = extract_bearer(&headers).ok_or(ApiError::Unauthorized(Some("missing token")))?;
    let user = db::validate_session(&state.db, token, now_ms())
        .await
        .map_err(|e| db_err("validate_session", e))?
        .ok_or(ApiError::Unauthorized(Some("invalid or expired session")))?;

    let mut answers = db::get_review_answers(&state.db, user.id)
        .await
        .map_err(|e| db_err("get_review_answers", e))?;
    let cards = db::get_all_cards(&state.db, &user.email)
        .await
        .map_err(|e| db_err("get_all_cards", e))?;
    let items = build_items(cards.into_iter().filter_map(|c| {
        Some(CardHistory {
            answers: answers.remove(&c.id)?,
            reps: usize::try_from(c.reps).unwrap_or(0),
        })
    }));
    if items.len() < MIN_ITEMS {
        return Err(ApiError::BadRequest(format!(
            "not enough review history yet: {} usable reviews, {MIN_ITEMS} needed",
            items.len()
        )));
    }
    let reviews = items.len();
    let permit = TRAINING
        .try_acquire()
        .map_err(|_| ApiError::TooManyRequests)?;
    // Moved in, so a dropped request still holds it until training ends.
    let params = run_blocking("fsrs optimize", move || {
        let _permit = permit;
        optimize(items)
    })
    .await?;
    Ok(Json(OptimizeResponse { params, reviews }))
}
