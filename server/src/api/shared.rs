//! Helpers shared by the endpoint modules: id/token generation, the
//! blocking-work wrapper for CPU-bound dictionary lookups, and uniform error
//! mapping.

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use rand::Rng;
use tracing::error;

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
/// fallible result into a single 500 response. The label is logged so it's
/// possible to tell which call failed without a stack trace. Used for the
/// CPU-bound in-memory dict lookups; DB calls go through sqlx directly.
pub(super) async fn run_blocking<F, T>(label: &'static str, f: F) -> Result<T, Response>
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
pub(super) fn db_err(op: &'static str, e: anyhow::Error) -> Response {
    error!(op, error = ?e, "db call failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
