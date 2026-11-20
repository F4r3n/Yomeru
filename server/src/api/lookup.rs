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
    http::HeaderMap,
    response::IntoResponse,
};
use examples_types::ExampleEntry;
use jmdict_types::ArchivedWordEntry;
use kanjidic_types::{KanjiEntry, PartInfo, PartSearch};
use serde::{Deserialize, Serialize};

use super::{ApiError, rate_limit, run_blocking};
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
/// The longest production answer checked: a sentence, not a text. The
/// check looks up words at every char, so it grows faster than the answer.
const MAX_ANSWER_CHARS: usize = 100;

/// More parts than any kanji holds: past this the answer is empty anyway.
const MAX_PARTS: usize = 16;

fn too_large(what: &str, got: usize, max: usize) -> ApiError {
    ApiError::BadRequest(format!("{what} too large: {got} (max {max})"))
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

#[derive(Serialize)]
pub struct KanjiPartsResponse {
    pub parts: Vec<PartInfo>,
}

#[derive(Deserialize)]
pub struct KanjiByPartsBody {
    /// The selected parts, one char each.
    pub parts: String,
    #[serde(default = "default_by_parts_max")]
    pub max: u8,
}

fn default_by_parts_max() -> u8 {
    100
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

#[derive(Deserialize)]
pub struct GrammarBody {
    pub text: String,
    #[serde(default = "default_grammar_max")]
    pub max: u8,
}

fn default_grammar_max() -> u8 {
    8
}

#[derive(Serialize)]
pub struct GrammarResponse {
    pub entries: Vec<grammar_core::GrammarEntry>,
}

#[derive(Serialize)]
pub struct GrammarBySeqResponse {
    pub results: Vec<Option<grammar_core::GrammarEntry>>,
}

#[derive(Deserialize)]
pub struct SentenceBody {
    pub text: String,
}

#[derive(Serialize)]
pub struct SentenceWord {
    pub start: usize,
    pub len: usize,
    pub entry: &'static ArchivedWordEntry,
}

#[derive(Serialize)]
pub struct SentenceWordsResponse {
    pub words: Vec<SentenceWord>,
}

/// One word of a sentence breakdown: where it is, in chars, its dictionary
/// form, how it is conjugated from it (`deinflect::Reason` names) and its
/// entry, if it has one.
#[derive(Serialize)]
pub struct BreakdownWord {
    pub start: usize,
    pub len: usize,
    pub base: String,
    pub reasons: Vec<&'static str>,
    pub entry: Option<&'static ArchivedWordEntry>,
}

/// One grammar point found in a sentence: where it is, its pattern's own
/// text as `[start, len]` pieces, and the points spelled that way.
#[derive(Serialize)]
pub struct BreakdownGrammar {
    pub start: usize,
    pub len: usize,
    pub pieces: Vec<(usize, usize)>,
    pub entries: Vec<grammar_core::GrammarEntry>,
}

#[derive(Serialize)]
pub struct SentenceBreakdownResponse {
    pub words: Vec<BreakdownWord>,
    pub grammar: Vec<BreakdownGrammar>,
    /// The expressions JMdict lists whole that `words` shows split
    /// (と言った as と|言った).
    pub phrases: Vec<BreakdownWord>,
}

#[derive(Deserialize)]
pub struct GrammarCheckBody {
    /// The point's id.
    pub id: String,
    pub answer: String,
}

#[derive(Serialize)]
pub struct GrammarCheckResponse {
    pub verdict: grammar_core::Verdict,
}

pub async fn lookup_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LookupBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let words = body.words;
    if words.len() > MAX_LOOKUP_WORDS {
        return Err(too_large("words", words.len(), MAX_LOOKUP_WORDS));
    }
    let results = run_blocking("lookup", move || {
        Ok(words.iter().map(|w| jmdict_core::lookup(w)).collect())
    })
    .await?;
    Ok(Json(LookupResponse { results }))
}

pub async fn lookup_by_sequence_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LookupBySequenceBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
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
    Ok(Json(LookupBySequenceResponse { results }))
}

pub async fn lookup_prefix_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LookupPrefixBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let text = body.text;
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("text", text.chars().count(), MAX_QUERY_CHARS));
    }
    let max = body.max;
    let results = run_blocking("lookup_prefix", move || {
        Ok(jmdict_core::lookup_prefix(&text, max))
    })
    .await?;
    Ok(Json(LookupPrefixResponse { results }))
}

pub async fn kanji_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<KanjiBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let word = body.word;
    if word.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("word", word.chars().count(), MAX_QUERY_CHARS));
    }
    let entries = run_blocking("kanji_lookup", move || {
        Ok(kanjidic_core::lookup_many(&word))
    })
    .await?;
    Ok(Json(KanjiResponse { entries }))
}

pub async fn kanji_parts_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let parts = run_blocking("kanji_parts", || Ok(kanjidic_core::parts())).await?;
    Ok(Json(KanjiPartsResponse { parts }))
}

pub async fn kanji_by_parts_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<KanjiByPartsBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let parts: Vec<char> = body.parts.chars().collect();
    if parts.len() > MAX_PARTS {
        return Err(too_large("parts", parts.len(), MAX_PARTS));
    }
    let max = usize::from(body.max);
    let result: PartSearch = run_blocking("kanji_by_parts", move || {
        Ok(kanjidic_core::kanji_with_parts(&parts, max))
    })
    .await?;
    Ok(Json(result))
}

pub async fn examples_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<ExamplesBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let word = body.word;
    if word.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("word", word.chars().count(), MAX_QUERY_CHARS));
    }
    let max = body.max as usize;
    let entries = run_blocking("examples_lookup", move || {
        Ok(examples_core::lookup(&word, max))
    })
    .await?;
    Ok(Json(ExamplesResponse { entries }))
}

/// The grammar points the Lookup page's search shows for `text`.
pub async fn grammar_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<GrammarBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let text = body.text;
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("text", text.chars().count(), MAX_QUERY_CHARS));
    }
    let max = usize::from(body.max);
    let entries = run_blocking("grammar_search", move || {
        Ok(crate::dicts::grammar()
            .map(|d| d.search(&text, max))
            .unwrap_or_default())
    })
    .await?;
    Ok(Json(GrammarResponse { entries }))
}

/// The points grammar cards with these `sequence`s review, aligned with the
/// request; `null` for a point that left the dictionary.
pub async fn grammar_by_sequence_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LookupBySequenceBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let sequences = body.sequences;
    if sequences.len() > MAX_LOOKUP_SEQUENCES {
        return Err(too_large(
            "sequences",
            sequences.len(),
            MAX_LOOKUP_SEQUENCES,
        ));
    }
    let results = run_blocking("grammar_by_sequence", move || {
        let dict = crate::dicts::grammar();
        Ok(sequences
            .iter()
            .map(|&s| dict.and_then(|d| d.by_seq(s)))
            .collect())
    })
    .await?;
    Ok(Json(GrammarBySeqResponse { results }))
}

/// Whether a typed production answer uses the point `id`. Only the web app
/// asks: it has no dictionaries of its own. The verdict is a help for the
/// user's rating, so it is never kept or checked again.
pub async fn grammar_check_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<GrammarCheckBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let GrammarCheckBody { id, answer } = body;
    let chars = answer.chars().count();
    if chars > MAX_ANSWER_CHARS {
        return Err(too_large("answer", chars, MAX_ANSWER_CHARS));
    }
    let verdict = run_blocking("grammar_check", move || {
        Ok(
            crate::dicts::grammar().map_or(grammar_core::Verdict::SelfGraded, |d| {
                d.check_answer(&id, &answer, &grammar_core::JmdictVocabulary)
            }),
        )
    })
    .await?;
    Ok(Json(GrammarCheckResponse { verdict }))
}

/// The words of a sentence, for a production card's vocabulary hint.
pub async fn sentence_words_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SentenceBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let text = body.text;
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("text", text.chars().count(), MAX_QUERY_CHARS));
    }
    let words = run_blocking("sentence_words", move || {
        Ok(jmdict_core::vocab::sentence_words(&text)
            .into_iter()
            .map(|(start, len, entry)| SentenceWord { start, len, entry })
            .collect::<Vec<_>>())
    })
    .await?;
    Ok(Json(SentenceWordsResponse { words }))
}

/// A sentence read into its words and the grammar they make up, for the
/// Lookup page's Sentence tab.
pub async fn sentence_breakdown_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SentenceBody>,
) -> Result<impl IntoResponse, ApiError> {
    rate_limit(&state, &state.lookup_limiter, addr, &headers)?;
    let text = body.text;
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(too_large("text", text.chars().count(), MAX_QUERY_CHARS));
    }
    let response = run_blocking("sentence_breakdown", move || {
        let Some(analyzer) = crate::dicts::analyzer() else {
            return Ok(SentenceBreakdownResponse {
                words: Vec::new(),
                grammar: Vec::new(),
                phrases: Vec::new(),
            });
        };
        let b = analyzer.analyze(&text);
        let word = |w: jmdict_core::segment::Word| BreakdownWord {
            start: w.start,
            len: w.len,
            reasons: sentence_core::reason_names(w.reasons),
            base: w.base,
            entry: w.entry,
        };
        Ok(SentenceBreakdownResponse {
            words: b.words.into_iter().map(word).collect(),
            phrases: b.phrases.into_iter().map(word).collect(),
            grammar: b
                .grammar
                .into_iter()
                .map(|m| BreakdownGrammar {
                    start: m.start,
                    len: m.len,
                    pieces: m.pieces,
                    entries: m.entries,
                })
                .collect(),
        })
    })
    .await?;
    Ok(Json(response))
}
