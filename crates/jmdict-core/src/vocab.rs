//! What grammar-core's answer check asks of the dictionary
//! (`grammar_core::Vocabulary`): whether a base form is a verb, and how
//! long the word at a position is. Kept here so the app, the server and the
//! extension answer alike.

use jmdict_types::ArchivedPartOfSpeech as P;

/// The longest word [`word_len_at`] tries, in chars.
const MAX_WORD_CHARS: usize = 20;

fn is_verb_pos(p: &P) -> bool {
    matches!(
        p,
        P::VerbIchidan
            | P::VerbIchidanS
            | P::VerbGodanBu
            | P::VerbGodanGu
            | P::VerbGodanKu
            | P::VerbGodanKuS
            | P::VerbGodanMu
            | P::VerbGodanNu
            | P::VerbGodanRu
            | P::VerbGodanRuIrr
            | P::VerbGodanSu
            | P::VerbGodanTsu
            | P::VerbGodanU
            | P::VerbGodanUS
            | P::VerbGodanUru
            | P::VerbSuru
            | P::VerbSuruS
            | P::VerbSuruC
            | P::VerbKuru
            | P::VerbNu
            | P::VerbRu
            | P::VerbUnclassified
    )
}

fn has_verb_sense(text: &str) -> bool {
    crate::lookup(text)
        .iter()
        .any(|e| e.senses.iter().any(|s| s.pos.iter().any(is_verb_pos)))
}

/// Whether `base`, a dictionary form, is a verb: a JMdict verb, or a noun
/// taking する written with it (勉強する).
pub fn is_verb(base: &str) -> bool {
    has_verb_sense(base)
        || base
            .strip_suffix("する")
            .is_some_and(|n| !n.is_empty() && has_verb_sense(n))
}

/// The length in chars of the longest word, conjugated or not, starting at
/// `char_offset` of `text`; 1 when there is none.
pub fn word_len_at(text: &str, char_offset: usize) -> usize {
    let Some(rest) = text
        .char_indices()
        .nth(char_offset)
        .and_then(|(i, _)| text.get(i..))
    else {
        return 1;
    };
    crate::lookup_longest_match(rest, MAX_WORD_CHARS).map_or(1, |(_, n)| n.max(1))
}

/// The words of `sentence` in order, for a production card's vocabulary
/// hint: `(start, len, entry)` in chars, the likeliest entry of each
/// ([`best_entry`]). Read
/// left to right taking the longest word at each point, as the hover popup
/// does; a char no word starts on is skipped.
pub fn sentence_words(
    sentence: &str,
) -> Vec<(usize, usize, &'static jmdict_types::ArchivedWordEntry)> {
    let starts: Vec<usize> = sentence.char_indices().map(|(i, _)| i).collect();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(&byte) = starts.get(at) {
        let found = sentence
            .get(byte..)
            .and_then(|rest| crate::lookup_longest_match(rest, MAX_WORD_CHARS));
        match found {
            Some((entries, len)) if len > 0 => {
                let surface: String = sentence.chars().skip(at).take(len).collect();
                if let Some(best) = best_entry(&entries, &surface) {
                    out.push((at, len, best));
                }
                at += len;
            }
            _ => at += 1,
        }
    }
    out
}

/// The entry a word of a sentence most likely is, of those its text
/// matched: one whose headword (or, for kana, reading) starts as the text
/// does (本 is ほん, not the 元 it can also spell), then a common one.
/// Ties keep the dictionary's order.
fn best_entry(
    entries: &[&'static jmdict_types::ArchivedWordEntry],
    surface: &str,
) -> Option<&'static jmdict_types::ArchivedWordEntry> {
    let first = surface.chars().next()?;
    let kana = japanese_utils::is_kana(first);
    let score = |e: &jmdict_types::ArchivedWordEntry| {
        let starts = |t: &str| t.starts_with(first);
        let written_so = starts(e.headword()) || (kana && starts(e.primary_reading()));
        let common = e.kanji_forms.iter().any(|k| !k.priorities.is_empty())
            || e.reading_forms.iter().any(|r| !r.priorities.is_empty());
        (written_so, common)
    };
    entries
        .iter()
        .copied()
        .enumerate()
        .max_by_key(|&(i, e)| (score(e), std::cmp::Reverse(i)))
        .map(|(_, e)| e)
}
