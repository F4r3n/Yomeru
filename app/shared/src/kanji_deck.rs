//! The kanji deck: one card per kanji appearing in a word under review.
//!
//! Kanji cards are never added by hand. Every time the Kanji review tab loads,
//! [`ensure_kanji_cards`] creates cards for any kanji the active/graduated words
//! use that don't have one yet. Reconciling on load (rather than hooking every
//! word mutation) picks up promotions made on other devices and imports too.
//! Kanji cards outlive the words that introduced them, so removing a word never
//! throws away kanji progress.

use std::collections::{HashMap, HashSet};

use japanese_utils::is_kanji;

use jmdict_types::WordEntry;

use crate::dict::{lookup_by_sequence, preferred_headword, primary_reading, short_gloss};
use crate::idb::{get_all_cards, put_cards};
use crate::srs::now_ms;
use crate::sync::schedule_sync;
use crate::types::{CardStatus, SrsCard};

/// A word under review, as far as the kanji deck cares.
#[derive(Debug, Clone, PartialEq)]
pub struct StudiedWord {
    /// The word's JMdict `ent_seq`; tells apart entries spelled alike, like
    /// 上手 じょうず and 上手 うわて.
    pub sequence: u32,
    /// When the word entered the deck; orders which kanji come up first.
    pub added_ms: f64,
    /// The written form being studied — the preferred headword, so a word
    /// usually written in kana contributes no kanji.
    pub headword: String,
    /// Kana reading, empty when the headword is already kana.
    pub reading: String,
    /// A reminder of the meaning, as [`short_gloss`] gives it.
    pub gloss: String,
}

impl StudiedWord {
    fn from_entry(entry: &WordEntry, added_ms: f64) -> Self {
        let headword = preferred_headword(entry).to_string();
        let reading = primary_reading(entry);
        Self {
            sequence: entry.sequence,
            added_ms,
            reading: if reading == headword {
                String::new()
            } else {
                reading.to_string()
            },
            headword,
            gloss: short_gloss(entry),
        }
    }
}

/// Cards for the kanji in `words` that have none yet, deduplicated, kanji from
/// the earliest-added words first. They are due now, staggered by a
/// millisecond each so a due-ordered session keeps that order.
pub fn missing_kanji_cards(
    words: &[StudiedWord],
    existing_ids: &HashSet<String>,
    now: f64,
) -> Vec<SrsCard> {
    let mut ordered: Vec<&StudiedWord> = words.iter().collect();
    ordered.sort_by(|a, b| a.added_ms.total_cmp(&b.added_ms));
    let mut seen = HashSet::new();
    ordered
        .into_iter()
        .flat_map(|w| w.headword.chars())
        .filter(|&c| is_kanji(c) && seen.insert(c))
        .map(|c| SrsCard::new_kanji(c, now))
        .filter(|card| !existing_ids.contains(&card.id))
        .enumerate()
        .map(|(i, mut card)| {
            card.due_ms = now + i as f64;
            card
        })
        .collect()
}

/// Each kanji mapped to the studied words containing it, in `words` order and
/// without repeats — the "Your words" list on the back of a kanji card.
pub fn words_by_kanji(words: &[StudiedWord]) -> HashMap<char, Vec<StudiedWord>> {
    let mut map: HashMap<char, Vec<StudiedWord>> = HashMap::new();
    for w in words {
        let mut in_word = HashSet::new();
        for c in w
            .headword
            .chars()
            .filter(|&c| is_kanji(c) && in_word.insert(c))
        {
            map.entry(c).or_default().push(w.clone());
        }
    }
    map
}

/// Creates the missing kanji cards for the words currently under review and
/// returns [`words_by_kanji`] for them. Schedules a sync when anything was
/// added.
pub async fn ensure_kanji_cards() -> Result<HashMap<char, Vec<StudiedWord>>, String> {
    let all = get_all_cards().await?;
    let existing_ids: HashSet<String> = all.iter().map(|c| c.id.clone()).collect();

    // One entry per word: its two direction cards share a sequence.
    let mut added: HashMap<u32, f64> = HashMap::new();
    for c in all
        .iter()
        .filter(|c| !c.is_kanji() && matches!(c.status, CardStatus::Active | CardStatus::Graduated))
    {
        let t = added.entry(c.sequence).or_insert(c.added_ms);
        *t = t.min(c.added_ms);
    }
    let sequences: Vec<u32> = added.keys().copied().collect();
    let entries = if sequences.is_empty() {
        vec![]
    } else {
        lookup_by_sequence(&sequences).await?
    };
    let words: Vec<StudiedWord> = sequences
        .iter()
        .zip(entries)
        .filter_map(|(seq, entry)| Some(StudiedWord::from_entry(&entry?, *added.get(seq)?)))
        .collect();

    let missing = missing_kanji_cards(&words, &existing_ids, now_ms());
    if !missing.is_empty() {
        put_cards(&missing).await?;
        schedule_sync();
    }
    Ok(words_by_kanji(&words))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CardDirection;

    fn word(added_ms: f64, headword: &str) -> StudiedWord {
        StudiedWord {
            sequence: 0,
            added_ms,
            headword: headword.into(),
            reading: String::new(),
            gloss: String::new(),
        }
    }

    fn kanji_of(cards: &[SrsCard]) -> String {
        cards.iter().filter_map(SrsCard::kanji_char).collect()
    }

    #[test]
    fn one_active_card_per_kanji_skipping_kana() {
        let cards = missing_kanji_cards(&[word(0.0, "食べる")], &HashSet::new(), 1_000.0);
        assert_eq!(kanji_of(&cards), "食");
        assert_eq!(cards[0].direction, CardDirection::Kanji);
        assert_eq!(cards[0].status, CardStatus::Active);
        assert_eq!(cards[0].id, format!("{}::kanji", u32::from('食')));
    }

    #[test]
    fn shared_kanji_get_one_card() {
        let cards = missing_kanji_cards(
            &[word(0.0, "学校"), word(1.0, "大学")],
            &HashSet::new(),
            0.0,
        );
        assert_eq!(kanji_of(&cards), "学校大");
    }

    #[test]
    fn earliest_added_words_come_first_and_stay_ordered_by_due() {
        let cards = missing_kanji_cards(
            &[word(5.0, "電車"), word(1.0, "猫")],
            &HashSet::new(),
            100.0,
        );
        assert_eq!(kanji_of(&cards), "猫電車");
        assert!(cards.windows(2).all(|w| w[0].due_ms < w[1].due_ms));
    }

    #[test]
    fn existing_cards_are_left_alone() {
        let existing: HashSet<String> = [SrsCard::new_kanji('学', 0.0).id].into();
        let cards = missing_kanji_cards(&[word(0.0, "学校")], &existing, 0.0);
        assert_eq!(kanji_of(&cards), "校");
    }

    #[test]
    fn kanji_ids_never_meet_word_ids() {
        // JMdict ent_seq starts at 1,000,000; the highest kanji is_kanji accepts
        // sits well below.
        assert!(u32::from('\u{2B81F}') < 1_000_000);
    }

    fn entry(kanji: &[&str], reading: &str, glosses: &[&str]) -> WordEntry {
        use jmdict_types::{Gloss, KanjiElement, ReadingElement, Sense};
        WordEntry {
            sequence: 1_000_000,
            kanji_forms: kanji.iter().map(|k| KanjiElement::from_text(*k)).collect(),
            reading_forms: vec![ReadingElement::from_reading(reading)],
            senses: vec![Sense {
                glosses: glosses.iter().map(|g| Gloss::new(*g, None)).collect(),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn studied_word_keeps_reading_and_a_short_gloss() {
        let w = StudiedWord::from_entry(
            &entry(
                &["学校"],
                "がっこう",
                &["school", "academy", "college", "seminary"],
            ),
            7.0,
        );
        assert_eq!(w.headword, "学校");
        assert_eq!(w.reading, "がっこう");
        assert_eq!(w.sequence, 1_000_000);
        assert_eq!(w.gloss, "school; academy");
        assert_eq!(w.added_ms, 7.0);
    }

    #[test]
    fn kana_headword_has_no_separate_reading() {
        let w = StudiedWord::from_entry(&entry(&[], "これ", &["this"]), 0.0);
        assert_eq!(w.headword, "これ");
        assert_eq!(w.reading, "");
    }

    #[test]
    fn words_by_kanji_lists_each_word_once() {
        let map = words_by_kanji(&[word(0.0, "学校"), word(1.0, "学生"), word(2.0, "人々")]);
        let headwords =
            |k: char| -> Vec<&str> { map[&k].iter().map(|w| w.headword.as_str()).collect() };
        assert_eq!(headwords('学'), ["学校", "学生"]);
        assert_eq!(headwords('人'), ["人々"]);
        assert!(!map.contains_key(&'々'));
    }
}
