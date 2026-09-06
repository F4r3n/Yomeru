//! Unauthenticated dictionary endpoints, rate-limited by the shared
//! `lookup_limiter`.
//!
//! Every handler bounds its input before touching the index: these take an
//! unauthenticated request body, so an unbounded list is a free denial of
//! service. The work itself is CPU-bound and in-memory, so it runs through
//! [`super::run_blocking`] rather than holding the async worker.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use examples_types::ExampleEntry;
use jmdict_types::ArchivedWordEntry;
use kanjidic_types::KanjiEntry;
use serde::{Deserialize, Serialize};

use super::run_blocking;
use crate::AppState;

// These endpoints are unauthenticated, so the request body is the one input an
// anonymous caller fully controls. Axum's 2 MB default body cap still allows
// tens of thousands of entries per request, and the response is far larger than
// the request that triggers it — so bound the batch explicitly.
//
// The limits are set above real client usage rather than at it: word_list
// resolves an entire deck in one call, so the sequence cap has to clear a large
// collection. Over-limit is a 400 rather than a truncation, because
// `lookup`/`lookup_by_sequence` results are positionally aligned with the
// request and a short response would silently misalign the client's mapping.
const MAX_LOOKUP_WORDS: usize = 1_000;

const MAX_LOOKUP_SEQUENCES: usize = 20_000;

const MAX_QUERY_CHARS: usize = 256;

fn too_large(what: &str, got: usize, max: usize) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": format!("{what} too large: {got} (max {max})")
        })),
    )
        .into_response()
}

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
    headers: HeaderMap,
    Json(body): Json<LookupBody>,
) -> Result<Response, Response> {
    if state
        .lookup_limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let words = body.words;
    if words.len() > MAX_LOOKUP_WORDS {
        return Err(too_large("words", words.len(), MAX_LOOKUP_WORDS));
    }
    let results = run_blocking("lookup", move || {
        Ok(words.iter().map(|w| jmdict_core::lookup(w)).collect())
    })
    .await?;
    Ok(Json(LookupResponse { results }).into_response())
}

pub async fn lookup_by_sequence_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LookupBySequenceBody>,
) -> Result<Response, Response> {
    if state
        .lookup_limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let sequences = body.sequences;
    if sequences.len() > MAX_LOOKUP_SEQUENCES {
        return Err(too_large(
            "sequences",
            sequences.len(),
            MAX_LOOKUP_SEQUENCES,
        ));
    }
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
    headers: HeaderMap,
    Json(body): Json<LookupPrefixBody>,
) -> Result<Response, Response> {
    if state
        .lookup_limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let text = body.text;
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("text", text.chars().count(), MAX_QUERY_CHARS));
    }
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
    headers: HeaderMap,
    Json(body): Json<KanjiBody>,
) -> Result<Response, Response> {
    if state
        .lookup_limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let word = body.word;
    if word.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("word", word.chars().count(), MAX_QUERY_CHARS));
    }
    let entries = run_blocking("kanji_lookup", move || {
        Ok(kanjidic_core::lookup_many(&word))
    })
    .await?;
    Ok(Json(KanjiResponse { entries }).into_response())
}

pub async fn examples_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<ExamplesBody>,
) -> Result<Response, Response> {
    if state
        .lookup_limiter
        .check_key(&state.client_ip(addr, &headers))
        .is_err()
    {
        return Err(StatusCode::TOO_MANY_REQUESTS.into_response());
    }
    let word = body.word;
    if word.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("word", word.chars().count(), MAX_QUERY_CHARS));
    }
    let max = body.max as usize;
    let entries = run_blocking("examples_lookup", move || {
        Ok(examples_core::lookup(&word, max))
    })
    .await?;
    Ok(Json(ExamplesResponse { entries }).into_response())
}
