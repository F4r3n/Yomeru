//! Helpers shared by the endpoint modules: id/token generation, the
//! blocking-work wrapper for CPU-bound dictionary lookups, rate limiting, and
//! the [`ApiError`] every handler fails with.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use governor::DefaultKeyedRateLimiter;
use rand::Rng;
use tracing::error;

use crate::AppState;

/// A failed request. Variants with a message send it as `{"error": "…"}`.
#[derive(Debug)]
pub(crate) enum ApiError {
    TooManyRequests,
    BadRequest(String),
    Unauthorized(Option<&'static str>),
    Forbidden(&'static str),
    /// A 500; the cause is logged where it happened.
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, None),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, Some(m)),
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.map(str::to_owned)),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, Some(m.to_owned())),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, None),
        };
        match msg {
            Some(m) => (status, Json(serde_json::json!({ "error": m }))).into_response(),
            None => status.into_response(),
        }
    }
}

/// Spends one unit of `limiter`'s quota for the request's client address.
pub(super) fn rate_limit(
    state: &AppState,
    limiter: &DefaultKeyedRateLimiter<IpAddr>,
    addr: SocketAddr,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    limiter
        .check_key(&state.client_ip(addr, headers))
        .map_err(|_| ApiError::TooManyRequests)
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(super) fn gen_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0u32..1_000_000))
}

pub(super) fn gen_token() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().r#gen()).collect();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

/// Runs `f` on a blocking thread and folds both panic (`JoinError`) and
/// fallible result into a single [`ApiError::Internal`]. The label is logged so it's
/// possible to tell which call failed without a stack trace. Used for the
/// CPU-bound in-memory dict lookups; DB calls go through sqlx directly.
pub(super) async fn run_blocking<F, T>(label: &'static str, f: F) -> Result<T, ApiError>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => {
            error!(op = label, error = ?e, "blocking call failed");
            Err(ApiError::Internal)
        }
        Err(e) => {
            error!(op = label, error = ?e, "blocking task panicked");
            Err(ApiError::Internal)
        }
    }
}

/// Logs a DB error with its operation label and returns a 500.
pub(super) fn db_err(op: &'static str, e: anyhow::Error) -> ApiError {
    error!(op, error = ?e, "db call failed");
    ApiError::Internal
}
