use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use examples_types::ExampleEntry;
use jmdict_types::ArchivedWordEntry;
use kanjidic_types::KanjiEntry;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

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

#[derive(Deserialize)]
pub struct SyncBody {
    pub cards: Vec<db::Card>,
    #[serde(default)]
    pub deletions: Vec<String>,
    /// Client's current scheduler settings. Optional so older clients that
    /// don't send settings keep working (cards-only sync).
    #[serde(default)]
    pub settings: Option<db::Settings>,
}

#[derive(Serialize)]
pub struct SyncResponse {
    pub cards: Vec<db::Card>,
    pub deletions: Vec<String>,
    /// The user's stored settings after the merge, or `None` if they've never
    /// synced any. Omitted from the JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<db::Settings>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn gen_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0u32..1_000_000))
}

fn gen_token() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().r#gen()).collect();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

/// Runs `f` on a blocking thread and folds both panic (`JoinError`) and
/// fallible result into a single 500 response. The label is logged so it's
/// possible to tell which call failed without a stack trace. Used for the
/// CPU-bound in-memory dict lookups; DB calls go through sqlx directly.
async fn run_blocking<F, T>(label: &'static str, f: F) -> Result<T, Response>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => {
            error!(op = label, error = ?e, "blocking call failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            error!(op = label, error = ?e, "blocking task panicked");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// Logs a DB error with its operation label and returns a 500 response.
fn db_err(op: &'static str, e: anyhow::Error) -> Response {
    error!(op, error = ?e, "db call failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

pub async fn auth_request_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<AuthRequestBody>,
) -> Result<Response, Response> {
    if state.limiter.check_key(&addr.ip()).is_err() {
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
    Json(body): Json<VerifyBody>,
) -> Result<Response, Response> {
    if state.limiter.check_key(&addr.ip()).is_err() {
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

pub async fn sync_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SyncBody>,
) -> Result<Response, Response> {
    if state.limiter.check_key(&addr.ip()).is_err() {
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

    db::apply_deletions(&state.db, &email, &body.deletions, now)
        .await
        .map_err(|e| db_err("apply_deletions", e))?;

    db::upsert_cards(&state.db, &email, &body.cards)
        .await
        .map_err(|e| db_err("upsert_cards", e))?;

    if let Some(ref s) = body.settings {
        db::upsert_settings(&state.db, &email, s)
            .await
            .map_err(|e| db_err("upsert_settings", e))?;
    }

    let merged = db::get_all_cards(&state.db, &email)
        .await
        .map_err(|e| db_err("get_all_cards", e))?;

    let tombstones = db::get_all_deletions(&state.db, &email)
        .await
        .map_err(|e| db_err("get_all_deletions", e))?;

    let settings = db::get_settings(&state.db, &email)
        .await
        .map_err(|e| db_err("get_settings", e))?;

    Ok(Json(SyncResponse {
        cards: merged,
        deletions: tombstones,
        settings,
    })
    .into_response())
}

// ---- Lookup endpoints (no auth, rate-limited by lookup_limiter) ----------

#[derive(Deserialize)]
pub struct LookupBody {
    pub words: Vec<String>,
}

#[derive(Serialize)]
pub struct LookupResponse {
    pub results: Vec<Vec<&'static ArchivedWordEntry>>,
}

#[derive(Deserialize)]
pub struct LookupBySequenceBody {
    pub sequences: Vec<u32>,
}

#[derive(Serialize)]
pub struct LookupBySequenceResponse {
    pub results: Vec<Option<&'static ArchivedWordEntry>>,
}

#[derive(Deserialize)]
pub struct LookupPrefixBody {
    pub text: String,
    #[serde(default = "default_prefix_max")]
    pub max: u8,
}

fn default_prefix_max() -> u8 {
    30
}

#[derive(Serialize)]
pub struct LookupPrefixResponse {
    pub results: Vec<&'static ArchivedWordEntry>,
}

#[derive(Deserialize)]
pub struct KanjiBody {
    pub word: String,
}

#[derive(Serialize)]
pub struct KanjiResponse {
    pub entries: Vec<KanjiEntry>,
}

#[derive(Deserialize)]
pub struct ExamplesBody {
    pub word: String,
    #[serde(default = "default_examples_max")]
    pub max: u8,
}

fn default_examples_max() -> u8 {
    5
}

#[derive(Serialize)]
pub struct ExamplesResponse {
    pub entries: Vec<ExampleEntry>,
}

pub async fn lookup_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LookupBody>,
) -> Result<Response, Response> {
    if state.lookup_limiter.check_key(&addr.ip()).is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let words = body.words;
    let results = run_blocking("lookup", move || {
        Ok(words.iter().map(|w| jmdict_core::lookup(w)).collect())
    })
    .await?;
    Ok(Json(LookupResponse { results }).into_response())
}

pub async fn lookup_by_sequence_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LookupBySequenceBody>,
) -> Result<Response, Response> {
    if state.lookup_limiter.check_key(&addr.ip()).is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let sequences = body.sequences;
    let results = run_blocking("lookup_by_sequence", move || {
        Ok(sequences
            .iter()
            .map(|s| jmdict_core::lookup_by_sequence(*s))
            .collect())
    })
    .await?;
    Ok(Json(LookupBySequenceResponse { results }).into_response())
}

pub async fn lookup_prefix_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LookupPrefixBody>,
) -> Result<Response, Response> {
    if state.lookup_limiter.check_key(&addr.ip()).is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let text = body.text;
    let max = body.max;
    let results = run_blocking("lookup_prefix", move || {
        Ok(jmdict_core::lookup_prefix(&text, max))
    })
    .await?;
    Ok(Json(LookupPrefixResponse { results }).into_response())
}

pub async fn kanji_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<KanjiBody>,
) -> Result<Response, Response> {
    if state.lookup_limiter.check_key(&addr.ip()).is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let word = body.word;
    let entries = run_blocking("kanji_lookup", move || {
        Ok(kanjidic_core::lookup_many(&word))
    })
    .await?;
    Ok(Json(KanjiResponse { entries }).into_response())
}

pub async fn examples_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<ExamplesBody>,
) -> Result<Response, Response> {
    if state.lookup_limiter.check_key(&addr.ip()).is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let word = body.word;
    let max = body.max as usize;
    let entries = run_blocking("examples_lookup", move || {
        Ok(examples_core::lookup(&word, max))
    })
    .await?;
    Ok(Json(ExamplesResponse { entries }).into_response())
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
            db_path: String::new(),
            data_dir: String::new(),
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
