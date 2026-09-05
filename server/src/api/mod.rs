//! HTTP endpoints, one module per concern.
//!
//! Handlers are re-exported here so `main.rs` keeps referring to `api::*`
//! regardless of which module a handler lives in.

mod auth;
mod lookup;
mod shared;
mod sync;

pub use auth::{auth_request_handler, auth_verify_handler};
pub use lookup::{
    examples_handler, kanji_handler, lookup_by_sequence_handler, lookup_handler,
    lookup_prefix_handler,
};
pub use sync::sync_handler;

use shared::{db_err, extract_bearer, gen_code, gen_token, now_ms, run_blocking};
