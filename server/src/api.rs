use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::db;
use crate::AppState;

#[derive(Deserialize)]
pub struct AuthRequestBody {
    pub email: String,
}

#[derive(Deserialize)]
pub struct VerifyBody {
    pub email: String,
    pub code: String,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    pub token: String,
}

#[derive(Deserialize)]
pub struct SyncBody {
    pub cards: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct SyncResponse {
    pub cards: Vec<serde_json::Value>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn gen_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0u32..1_000_000))
}

fn gen_token() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

pub async fn auth_request_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<AuthRequestBody>,
) -> impl IntoResponse {
    if state.limiter.check_key(&addr.ip()).is_err() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    let email = body.email.trim().to_lowercase();
    if email.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let code = gen_code();
    let now = now_ms();

    let db = state.db.clone();
    let email_clone = email.clone();
    let code_clone = code.clone();
    let stored = tokio::task::spawn_blocking(move || db::store_otp(&db, &email_clone, &code_clone, now))
        .await
        .unwrap();

    if stored.is_err() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    if let Err(e) = send_otp_email(&state.cfg, &email, &code).await {
        eprintln!("[yomeru-server] email send failed: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn auth_verify_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<VerifyBody>,
) -> impl IntoResponse {
    if state.limiter.check_key(&addr.ip()).is_err() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    let email = body.email.trim().to_lowercase();
    let code = body.code.trim().to_string();
    let now = now_ms();

    let db = state.db.clone();
    let email_clone = email.clone();
    let valid = tokio::task::spawn_blocking(move || db::verify_otp(&db, &email_clone, &code, now))
        .await
        .unwrap();

    if !valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid or expired code" })),
        )
            .into_response();
    }

    let token = gen_token();
    let expires_at = now + 30 * 24 * 3_600_000_i64;

    let db = state.db.clone();
    let token_clone = token.clone();
    tokio::task::spawn_blocking(move || db::create_session(&db, &token_clone, &email, expires_at))
        .await
        .unwrap();

    Json(VerifyResponse { token }).into_response()
}

pub async fn sync_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SyncBody>,
) -> impl IntoResponse {
    if state.limiter.check_key(&addr.ip()).is_err() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    let token = match extract_bearer(&headers) {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "missing token" })),
            )
                .into_response()
        }
    };

    let now = now_ms();
    let db = state.db.clone();
    let valid = tokio::task::spawn_blocking(move || db::validate_session(&db, &token, now))
        .await
        .unwrap();

    if valid.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid or expired session" })),
        )
            .into_response();
    }

    let db = state.db.clone();
    let cards = body.cards.clone();
    tokio::task::spawn_blocking(move || db::upsert_cards(&db, &cards))
        .await
        .unwrap();

    let db = state.db.clone();
    let merged = tokio::task::spawn_blocking(move || db::get_all_cards(&db))
        .await
        .unwrap();

    Json(SyncResponse { cards: merged }).into_response()
}

async fn send_otp_email(cfg: &Config, to: &str, code: &str) -> anyhow::Result<()> {
    use lettre::{
        transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport,
        Message, Tokio1Executor,
    };

    let from_mailbox: lettre::message::Mailbox =
        format!("Yomeru <{}>", cfg.smtp_from).parse()?;
    let to_mailbox: lettre::message::Mailbox = to.parse()?;

    let email = Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject("Yomeru sync code")
        .body(format!(
            "Your Yomeru verification code: {code}\n\nValid for 10 minutes."
        ))?;

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp_host)?
        .port(cfg.smtp_port);

    if let (Some(user), Some(pass)) = (&cfg.smtp_user, &cfg.smtp_pass) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }

    builder.build().send(email).await?;
    Ok(())
}
