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
use jmdict_types::{Freq, KanjiInf, Misc, WordEntry};
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

pub async fn examples_for(word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
    consume_context::<Platform>()
        .dict
        .examples_for(word, max)
        .await
}

pub fn primary_headword(e: &WordEntry) -> &str {
    e.kanji_forms
        .first()
        .map(|k| k.text.as_str())
        .or_else(|| e.reading_forms.first().map(|r| r.text.as_str()))
        .unwrap_or_default()
}

pub fn primary_reading(e: &WordEntry) -> &str {
    e.reading_forms
        .first()
        .map(|r| r.text.as_str())
        .unwrap_or_default()
}

/// Headword to display as the title of the entry card.
///
/// Returns the first kana reading instead of the kanji form when:
///   1. any sense carries the `uk` misc tag (usually written in kana), or
///   2. the only kanji form is tagged `rK`/`sK` (rare/search-only), or
///   3. the highest-priority reading outranks the highest-priority kanji form
///      under `priority_score`.
///
/// Otherwise returns the first kanji form (matching `primary_headword`).
pub fn preferred_headword(e: &WordEntry) -> &str {
    let Some(kanji) = e.kanji_forms.first() else {
        return primary_headword(e);
    };
    let Some(reading) = e.reading_forms.first() else {
        return kanji.text.as_str();
    };

    let usually_kana = e.senses.iter().any(|s| s.misc.contains(&Misc::UsuallyKana));
    let only_rare_kanji = e.kanji_forms.iter().all(|k| {
        k.info
            .iter()
            .any(|i| *i == KanjiInf::RareKanji || *i == KanjiInf::SearchOnlyKanji)
    });

    if usually_kana || only_rare_kanji {
        return reading.text.as_str();
    }

    if priority_score(&reading.priorities) < priority_score(&kanji.priorities) {
        return reading.text.as_str();
    }
    kanji.text.as_str()
}

/// Lower is more common. `u32::MAX` means no priority tags at all.
///
/// `nfXX` buckets are 500 words each: `nf01` = top 500, `nf48` = ranks
/// 23,501–24,000. Tier-1 tags (`news1`, `ichi1`, `spec1`, `gai1`) are scored
/// as 1; tier-2 (`news2`, `ichi2`, `spec2`, `gai2`) as 24 — i.e. roughly
/// "outside the top 12k".
pub fn priority_score(tags: &[Freq]) -> u32 {
    let mut best = u32::MAX;
    for t in tags {
        best = match t.kind {
            //1-48
            jmdict_types::FreqKind::Nf => best.min(u32::from(t.value)),
            //1-2
            jmdict_types::FreqKind::News
            | jmdict_types::FreqKind::Gai
            | jmdict_types::FreqKind::Ichi
            | jmdict_types::FreqKind::Spec => {
                if t.value == 1 {
                    best.min(1)
                } else {
                    best.min(24)
                }
            }
        };
    }
    best
}

/// Human-readable frequency band derived from the entry's JMdict priority
/// tags (`nfXX` buckets of 500 words + the tier-1/tier-2 corpus tags).
/// Returns `None` when the entry carries no priority tags at all.
pub fn frequency_label(e: &WordEntry) -> Option<&'static str> {
    let best = e
        .kanji_forms
        .iter()
        .map(|k| priority_score(&k.priorities))
        .chain(
            e.reading_forms
                .iter()
                .map(|r| priority_score(&r.priorities)),
        )
        .min()
        .unwrap_or(u32::MAX);
    match best {
        u32::MAX => None,
        0..=2 => Some("Top 1k"),
        3..=10 => Some("Top 5k"),
        11..=24 => Some("Common"),
        _ => Some("Uncommon"),
    }
}

pub fn direction_label(d: CardDirection) -> &'static str {
    match d {
        CardDirection::Recognition => "Recognition",
        CardDirection::Recall => "Recall",
    }
}
