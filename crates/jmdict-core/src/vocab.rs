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
/// hint: `(start, len, entry)` in chars. Read by [`crate::segment`], the
/// likeliest split of the whole sentence; text no dictionary word covers
/// is left out.
pub fn sentence_words(
    sentence: &str,
) -> Vec<(usize, usize, &'static jmdict_types::ArchivedWordEntry)> {
    crate::segment::segment(sentence)
        .into_iter()
        .filter_map(|w| Some((w.start, w.len, w.entry?)))
        .collect()
}
