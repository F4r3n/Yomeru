//! Free-function shims that delegate to the [`crate::platform::DictClient`]
//! in Dioxus context. The HTTP implementation lives in `platform.rs`; the
//! extension provides its own implementation that messages the background
//! script.
//!
//! All call sites in `routes/*` reach these from inside a `#[component]`
//! body via `spawn(...)`, where Dioxus' runtime scope is in-scope and
//! `consume_context` resolves correctly.

use dioxus::prelude::consume_context;
use examples_types::ExampleEntry;
use jmdict_types::WordEntry;
use kanjidic_types::KanjiEntry;

use crate::platform::Platform;
use crate::types::CardDirection;

/// Exact lookup of a single headword/reading.
pub async fn lookup(word: &str) -> Result<Vec<WordEntry>, String> {
    consume_context::<Platform>().dict.lookup(word).await
}

/// Batched exact lookup. One round-trip per call regardless of words.len().
pub async fn lookup_many(words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
    consume_context::<Platform>().dict.lookup_many(words).await
}

pub async fn lookup_prefix(text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
    consume_context::<Platform>().dict.lookup_prefix(text, max).await
}

pub async fn kanji_for(word: &str) -> Result<Vec<KanjiEntry>, String> {
    consume_context::<Platform>().dict.kanji_for(word).await
}

pub async fn examples_for(word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
    consume_context::<Platform>().dict.examples_for(word, max).await
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
