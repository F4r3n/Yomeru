//! Account lifecycle beyond sign-in: erasing an account (GDPR art. 17) and the
//! operator details the clients' privacy page shows.

use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use super::auth::send_code_email;
use super::{db_err, extract_bearer, gen_code, now_ms};
use crate::AppState;
use crate::db;

/// The account behind the request's Bearer token, after the strict per-IP
/// limit both deletion steps share: they end in a code check, so they get the
/// sign-in routes' quota.
async fn authed_user(
    state: &AppState,
    addr: SocketAddr,
    headers: &HeaderMap,
) -> Result<db::SessionUser, Response> {
    if state
        .limiter
        .check_key(&state.client_ip(addr, headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let Some(token) = extract_bearer(headers) else {
        return Err(StatusCode::UNAUTHORIZED.into_response());
    };
    db::validate_session(&state.db, token, now_ms())
        .await
        .map_err(|e| db_err("validate_session", e))?
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())
}

/// `POST /api/account/delete/request`, Bearer token: emails the account a
/// one-time code that [`delete_account_handler`] requires. A session alone is
/// not enough to erase an account — it may come from a stolen token or a
/// device left unlocked — so deletion also proves access to the mailbox.
///
/// Uses the sign-in code store, so the same TTL, attempt limit and resend
/// floor apply (429 while a recent code is still valid). In dev mode nothing
/// is emailed: the code comes back as `{"dev_code": "…"}`.
pub async fn delete_account_request_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let user = authed_user(&state, addr, &headers).await?;
    let code = gen_code();
    let stored = db::store_otp(&state.db, &user.email, &code, now_ms())
        .await
        .map_err(|e| db_err("store_otp", e))?;
    if !stored {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    if state.cfg.dev_mode {
        return Ok(Json(serde_json::json!({ "dev_code": code })).into_response());
    }
    let text = format!(
        "Your Yomeru account deletion code: {code}\n\n\
         Entering it erases your account and all your data on the sync server. \
         Valid for 10 minutes.\n\n\
         If you didn't ask for this, ignore this email: nothing is deleted without the code."
    );
    if let Err(e) = send_code_email(
        &state.cfg,
        &user.email,
        "Yomeru account deletion code",
        text,
    )
    .await
    {
        error!(user_id = user.id, error = ?e, "deletion code email failed");
        return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct DeleteBody {
    pub code: String,
}

/// `POST /api/account/delete`, Bearer token + `{"code": "…"}` from
/// [`delete_account_request_handler`]. Erases the account and all its
/// server-side data; every session of it, on every device, stops working.
/// 204 on success, 403 for a wrong or expired code.
pub async fn delete_account_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<DeleteBody>,
) -> Result<Response, Response> {
    let user = authed_user(&state, addr, &headers).await?;
    let valid = db::verify_otp(&state.db, &user.email, body.code.trim(), now_ms())
        .await
        .map_err(|e| db_err("verify_otp", e))?;
    if !valid {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "invalid or expired code" })),
        )
            .into_response());
    }

    db::delete_account(&state.db, user.id, &user.email)
        .await
        .map_err(|e| db_err("delete_account", e))?;
    // The id only: logging the address would keep it in the logs after the
    // account itself is gone.
    info!(user_id = user.id, "account deleted");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Serialize)]
pub struct PrivacyInfo {
    /// Who runs this server (GDPR data controller). `None` when unset.
    pub controller: Option<String>,
    /// Where to send data requests. `None` when unset.
    pub contact: Option<String>,
}

/// `GET` (or `POST`) `/api/privacy`: the operator details set at deploy time
/// (`YOMERU_PRIVACY_CONTROLLER`, `YOMERU_PRIVACY_CONTACT`). Public.
pub async fn privacy_handler(State(state): State<AppState>) -> Json<PrivacyInfo> {
    Json(PrivacyInfo {
        controller: state.cfg.privacy_controller.clone(),
        contact: state.cfg.privacy_contact.clone(),
    })
}
