//! Free-function shims that delegate to the [`crate::platform::DictClient`]
//! in Dioxus context, plus the entry helpers from `yomeru_core::dict`. The
//! HTTP implementation lives in `yomeru_core::platform`; the extension
//! provides its own implementation that messages the background script.
//!
//! All call sites in `routes/*` reach these from inside a `#[component]`
//! body via `spawn(...)`, where Dioxus' runtime scope is in-scope and
//! `consume_context` resolves correctly.

use dioxus::prelude::consume_context;
use examples_types::ExampleEntry;
use jmdict_types::WordEntry;
use kanjidic_types::{KanjiEntry, PartInfo, PartSearch};

pub use yomeru_core::dict::*;

use crate::platform::Platform;

/// Exact lookup of a single headword/reading.
pub async fn lookup(word: &str) -> Result<Vec<WordEntry>, String> {
    consume_context::<Platform>().dict.lookup(word).await
}

/// Batched exact lookup. One round-trip per call regardless of words.len().
pub async fn lookup_many(words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
    consume_context::<Platform>().dict.lookup_many(words).await
}

/// Look up entries by JMdict ent_seq. Returns one slot per requested
/// sequence, with `None` for sequences not found in the dictionary
/// (e.g. cards added before a JMdict update that retired the entry).
pub async fn lookup_by_sequence(sequences: &[u32]) -> Result<Vec<Option<WordEntry>>, String> {
    consume_context::<Platform>()
        .dict
        .lookup_by_sequence(sequences)
        .await
}

pub async fn lookup_prefix(text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
    consume_context::<Platform>()
        .dict
        .lookup_prefix(text, max)
        .await
}

pub async fn kanji_for(word: &str) -> Result<Vec<KanjiEntry>, String> {
    consume_context::<Platform>().dict.kanji_for(word).await
}

/// Every part the kanji finder offers.
pub async fn kanji_parts() -> Result<Vec<PartInfo>, String> {
    consume_context::<Platform>().dict.kanji_parts().await
}

/// The kanji holding every char of `parts`, at most `max` of them.
pub async fn kanji_with_parts(parts: &str, max: u8) -> Result<PartSearch, String> {
    consume_context::<Platform>()
        .dict
        .kanji_with_parts(parts, max)
        .await
}

pub async fn examples_for(word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
    consume_context::<Platform>()
        .dict
        .examples_for(word, max)
        .await
}
