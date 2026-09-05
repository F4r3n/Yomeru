//! Email one-time-code login: request a code, verify it, receive a session
//! token. The token is returned to the client and only ever stored hashed —
//! see [`crate::db::create_session`].

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use super::{db_err, gen_code, gen_token, now_ms};
use crate::AppState;
use crate::config::Config;
use crate::db;

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

pub async fn auth_request_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<AuthRequestBody>,
) -> Result<Response, Response> {
    if state
        .limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }

    let email = body.email.trim().to_lowercase();
    if email.is_empty() {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }

    // Dev mode: skip OTP+SMTP entirely. Issue a session token now and
    // hand it to the client so the UI can authenticate in one step.
    if state.cfg.dev_mode {
        info!(%email, "dev mode: auto-issuing token");
        let token = gen_token();
        let expires_at = now_ms() + 30 * 24 * 3_600_000_i64;
        db::create_session(&state.db, &token, &email, expires_at)
            .await
            .map_err(|e| db_err("dev_mode_create_session", e))?;
        return Ok(Json(VerifyResponse { token }).into_response());
    }

    let code = gen_code();
    let now = now_ms();

    let stored = db::store_otp(&state.db, &email, &code, now)
        .await
        .map_err(|e| db_err("store_otp", e))?;

    if !stored {
        // Per-email cooldown is still active.
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }

    if let Err(e) = send_otp_email(&state.cfg, &email, &code).await {
        error!(%email, error = ?e, "email send failed");
        return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn auth_verify_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<VerifyBody>,
) -> Result<Response, Response> {
    if state
        .limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }

    let email = body.email.trim().to_lowercase();
    let code = body.code.trim().to_string();
    let now = now_ms();

    let valid = db::verify_otp(&state.db, &email, &code, now)
        .await
        .map_err(|e| db_err("verify_otp", e))?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid or expired code" })),
        )
            .into_response());
    }

    let token = gen_token();
    let expires_at = now + 30 * 24 * 3_600_000_i64;

    db::create_session(&state.db, &token, &email, expires_at)
        .await
        .map_err(|e| db_err("create_session", e))?;

    Ok(Json(VerifyResponse { token }).into_response())
}

async fn send_otp_email(cfg: &Config, to: &str, code: &str) -> anyhow::Result<()> {
    use lettre::{
        AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
        transport::smtp::authentication::Credentials,
    };

    let from_mailbox: lettre::message::Mailbox = format!("Yomeru <{}>", cfg.smtp_from).parse()?;
    let to_mailbox: lettre::message::Mailbox = to.parse()?;

    let email = Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject("Yomeru sync code")
        .body(format!(
            "Your Yomeru verification code: {code}\n\nValid for 10 minutes."
        ))?;

    // Port 465 = SMTPS (implicit TLS); 587 (and others) use STARTTLS.
    let mut builder = if cfg.smtp_port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.smtp_host)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp_host)?
    }
    .port(cfg.smtp_port);

    if let (Some(user), Some(pass)) = (&cfg.smtp_user, &cfg.smtp_pass) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }

    builder.build().send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Manually-run integration test prints status to stdout (`--nocapture`).
    #![allow(clippy::print_stdout)]
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Parse a simple KEY=VALUE .env file (ignores blanks and `#` comments).
    fn load_dotenv(path: &PathBuf) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        let Ok(text) = fs::read_to_string(path) else {
            return map;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        map
    }

    /// Live SMTP test against the credentials in `server/.env`.
    ///
    /// Run with:
    ///   cargo test -p server -- --ignored smtp_send_real_email --nocapture
    ///
    /// Recipient is read from `SMTP_TEST_TO` in `server/.env` (process env wins).
    #[tokio::test]
    #[ignore]
    async fn smtp_send_real_email() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let env = load_dotenv(&manifest_dir.join(".env"));

        let get = |k: &str| -> Option<String> {
            std::env::var(k)
                .ok()
                .or_else(|| env.get(k).cloned())
                .filter(|s| !s.is_empty())
        };

        let cfg = Config {
            port: 0,
            bind: "127.0.0.1".into(),
            db_path: String::new(),
            data_dir: String::new(),
            trust_proxy: crate::client_ip::TrustProxy::Private,
            proxy_hops: 1,
            smtp_host: get("YOMERU_SMTP_HOST").expect("YOMERU_SMTP_HOST missing"),
            smtp_port: get("YOMERU_SMTP_PORT")
                .and_then(|s| s.parse().ok())
                .expect("YOMERU_SMTP_PORT missing or invalid"),
            smtp_from: get("YOMERU_SMTP_FROM").expect("YOMERU_SMTP_FROM missing"),
            smtp_user: get("YOMERU_SMTP_USER"),
            smtp_pass: get("YOMERU_SMTP_PASS"),
            dev_mode: false,
        };

        let to = get("SMTP_TEST_TO").expect("SMTP_TEST_TO missing in server/.env");

        println!(
            "sending test OTP to {to} via {}:{} as {}",
            cfg.smtp_host, cfg.smtp_port, cfg.smtp_from
        );

        send_otp_email(&cfg, &to, "123456")
            .await
            .expect("send_otp_email failed");
    }
}
