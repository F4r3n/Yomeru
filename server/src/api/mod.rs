//! HTTP endpoints, one module per concern.
//!
//! Handlers are re-exported here so `main.rs` keeps referring to `api::*`
//! regardless of which module a handler lives in.

mod account;
mod auth;
#[cfg(test)]
mod integration;
mod lookup;
#[cfg(test)]
mod lookup_integration;
mod shared;
mod sync;

pub use account::{delete_account_handler, delete_account_request_handler, privacy_handler};
pub use auth::{auth_request_handler, auth_verify_handler};
pub use lookup::{
    examples_handler, kanji_by_parts_handler, kanji_handler, kanji_parts_handler,
    lookup_by_sequence_handler, lookup_handler, lookup_prefix_handler,
};
pub use sync::sync_handler;

use shared::{db_err, extract_bearer, gen_code, gen_token, now_ms, run_blocking};

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::AppState;

/// The complete route table. Lives here rather than in `main` so tests can
/// drive the same wiring the binary serves — a router assembled separately in
/// a test proves nothing about the one that ships.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/auth/request", post(auth_request_handler))
        .route("/api/auth/verify", post(auth_verify_handler))
        .route("/api/sync", post(sync_handler))
        .route(
            "/api/account/delete/request",
            post(delete_account_request_handler),
        )
        .route("/api/account/delete", post(delete_account_handler))
        // GET for browsers and links; POST too, since the clients' HTTP
        // helper only speaks POST.
        .route("/api/privacy", get(privacy_handler).post(privacy_handler))
        .route("/api/lookup", post(lookup_handler))
        .route("/api/lookup-by-sequence", post(lookup_by_sequence_handler))
        .route("/api/lookup-prefix", post(lookup_prefix_handler))
        .route("/api/kanji", post(kanji_handler))
        .route("/api/kanji-parts", post(kanji_parts_handler))
        .route("/api/kanji-by-parts", post(kanji_by_parts_handler))
        .route("/api/examples", post(examples_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
