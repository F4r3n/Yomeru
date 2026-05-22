//! Dict lookup over HTTP. All lookups go to the yomeru-server
//! (`/api/lookup`, `/api/lookup-prefix`, `/api/kanji`, `/api/examples`).
//!
//! No in-process dict state — the server holds the FST + entry blob.

use examples_types::ExampleEntry;
use gloo_net::http::Request;
use jmdict_types::WordEntry;
use kanjidic_types::KanjiEntry;
use serde::{Deserialize, Serialize};

use crate::settings::default_server_url;
use crate::types::CardDirection;

/// Absolute API URL. Debug builds resolve to `http://127.0.0.1:4500/api/...`
/// (direct cross-origin, no dx proxy needed); release builds resolve to the
/// page origin so nginx routes /api/* via same-origin.
fn api_url(path: &str) -> String {
    let base = default_server_url();
    if base.is_empty() {
        return path.to_string();
    }
    format!("{}{}", base.trim_end_matches('/'), path)
}

#[derive(Serialize)]
struct LookupBody<'a> {
    words: &'a [String],
}

#[derive(Deserialize)]
struct LookupResponse {
    results: Vec<Vec<WordEntry>>,
}

#[derive(Serialize)]
struct LookupPrefixBody<'a> {
    text: &'a str,
    max: u8,
}

#[derive(Deserialize)]
struct LookupPrefixResponse {
    results: Vec<WordEntry>,
}

#[derive(Serialize)]
struct WordBody<'a> {
    word: &'a str,
}

#[derive(Serialize)]
struct WordMaxBody<'a> {
    word: &'a str,
    max: u8,
}

#[derive(Deserialize)]
struct KanjiResponse {
    entries: Vec<KanjiEntry>,
}

#[derive(Deserialize)]
struct ExamplesResponse {
    entries: Vec<ExampleEntry>,
}

/// Exact lookup of a single headword/reading.
pub async fn lookup(word: &str) -> Result<Vec<WordEntry>, String> {
    let mut results = lookup_many(&[word.to_owned()]).await?;
    Ok(results.pop().unwrap_or_default())
}

/// Batched exact lookup. One round-trip per call regardless of words.len().
pub async fn lookup_many(words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
    let body = LookupBody { words };
    let res = Request::post(&api_url("/api/lookup"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    let parsed: LookupResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.results)
}

pub async fn lookup_prefix(text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
    let body = LookupPrefixBody { text, max };
    let res = Request::post(&api_url("/api/lookup-prefix"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    let parsed: LookupPrefixResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.results)
}

pub async fn kanji_for(word: &str) -> Result<Vec<KanjiEntry>, String> {
    let body = WordBody { word };
    let res = Request::post(&api_url("/api/kanji"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    let parsed: KanjiResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.entries)
}

pub async fn examples_for(word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
    let body = WordMaxBody { word, max };
    let res = Request::post(&api_url("/api/examples"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    let parsed: ExamplesResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.entries)
}

pub fn primary_headword(e: &WordEntry) -> &str {
    e.kanji_forms
        .first()
        .map(|k| k.text.as_str())
        .or_else(|| e.reading_forms.first().map(|r| r.text.as_str()))
        .unwrap_or("")
}

pub fn primary_reading(e: &WordEntry) -> &str {
    e.reading_forms
        .first()
        .map(|r| r.text.as_str())
        .unwrap_or("")
}

pub fn direction_label(d: CardDirection) -> &'static str {
    match d {
        CardDirection::Recognition => "Recognition",
        CardDirection::Recall => "Recall",
    }
}
