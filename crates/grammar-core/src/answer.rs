//! The production review's answer check: does a typed sentence use the
//! card's point? See "Validation" in `docs/phase-4-grammar-cards.md`.
//!
//! Only the point is checked; the rest of the sentence is the user's to
//! judge against the reference. The words around it come from the caller's
//! dictionary ([`Vocabulary`]), so this crate stays free of JMdict.

use deinflect::{Reason, Reasons};
use serde::{Deserialize, Serialize};

use crate::dict::{GrammarDict, MAX_PATTERN_CHARS};
use crate::types::{AnswerCheck, GrammarEntry, Inflection, Related};

/// The longest stretch of an answer tried as one inflected verb, in chars.
const MAX_VERB_CHARS: usize = 12;

/// What the check needs to know about words, from the caller's dictionary
/// (JMdict in the apps).
pub trait Vocabulary {
    /// Whether `base`, a dictionary form the deinflector reached, is a verb:
    /// 食べる yes, 舞う yes, まう only as 舞う's reading. Rules alone would
    /// make an imperative of every え (まえ → まう).
    fn is_verb(&self, base: &str) -> bool;

    /// The length in chars of the longest word starting at `char_offset` of
    /// `text`, conjugated or not; 1 when there is none. Keeps a one-char
    /// point from being found inside a word: the さ of さん, the 中 of 中国.
    fn word_len_at(&self, text: &str, char_offset: usize) -> usize;
}

/// [`Vocabulary`] over the loaded jmdict-core dictionary
/// (`jmdict_core::vocab`).
#[cfg(feature = "jmdict")]
pub struct JmdictVocabulary;

#[cfg(feature = "jmdict")]
impl Vocabulary for JmdictVocabulary {
    fn is_verb(&self, base: &str) -> bool {
        jmdict_core::vocab::is_verb(base)
    }
    fn word_len_at(&self, text: &str, char_offset: usize) -> usize {
        jmdict_core::vocab::word_len_at(text, char_offset)
    }
}

/// The outcome of [`GrammarDict::check_answer`]. Serialized
/// `{"kind": "close", "related": …}`, to travel from the server or the
/// extension's background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Verdict {
    /// The answer uses the card's point.
    Accepted,
    /// The answer uses a point of the card's `related` list instead: "this
    /// works too, but the card practises…". A free retry.
    Close { related: Related },
    /// The answer uses other points (two chars or more), which the hints
    /// name: "You used てください."
    Other { found: Vec<Related> },
    /// None of the above.
    NotFound,
    /// The point is graded by the user (`check = "self"`), or the card's
    /// point is not in the dictionary any more.
    SelfGraded,
}

impl GrammarDict {
    /// Checks a typed production answer for the point `id`.
    pub fn check_answer(&self, id: &str, answer: &str, vocab: &impl Vocabulary) -> Verdict {
        let Some(entry) = self.by_id(id) else {
            return Verdict::SelfGraded;
        };
        if entry.check == AnswerCheck::SelfGraded {
            return Verdict::SelfGraded;
        }
        let answer = normalize(answer);
        let plain = plain_register(&answer);
        let uses = |e: &GrammarEntry| {
            [&answer, &plain]
                .iter()
                .any(|a| self.uses(e, a, vocab) == Some(true))
        };
        if uses(&entry) {
            return Verdict::Accepted;
        }
        for r in &entry.related {
            let used = self.by_id(&r.id).is_some_and(|e| uses(&e));
            if used {
                return Verdict::Close { related: r.clone() };
            }
        }
        let found: Vec<Related> = self
            .points_in(&answer)
            .into_iter()
            .filter(|e| e.id != entry.id)
            .map(|e| Related {
                id: e.id,
                pattern: e.pattern,
            })
            .collect();
        if found.is_empty() {
            Verdict::NotFound
        } else {
            Verdict::Other { found }
        }
    }

    /// Whether `answer` uses `entry`'s point; `None` for a self-graded one.
    fn uses(&self, entry: &GrammarEntry, answer: &str, vocab: &impl Vocabulary) -> Option<bool> {
        match &entry.check {
            AnswerCheck::SelfGraded => None,
            AnswerCheck::Inflection { forms } => {
                Some(forms.iter().any(|&f| has_inflection(answer, f, vocab)))
            }
            AnswerCheck::Forms => Some(entry.keys().any(|key| self.has_key(key, answer, vocab))),
        }
    }

    /// Whether the pattern or form `key` is in `answer`. A 〜 splits it into
    /// pieces found in order, each written as is or conjugated
    /// (しか〜ない: しか見ません). A one-char key must stand as its own
    /// word ([`Self::stands_alone`]).
    fn has_key(&self, key: &str, answer: &str, vocab: &impl Vocabulary) -> bool {
        let chars: Vec<char> = answer.chars().collect();
        let pieces: Vec<&str> = key.split('〜').filter(|p| !p.is_empty()).collect();
        let single = pieces.len() == 1 && key.chars().count() == 1;
        let mut from = 0;
        for piece in pieces {
            let found = (from..chars.len()).find_map(|start| {
                let end = piece_at(piece, &chars, start)?;
                (!single || self.stands_alone(answer, start, vocab)).then_some(end)
            });
            match found {
                Some(end) => from = end,
                None => return false,
            }
        }
        true
    }

    /// Whether the one-char key at `at` is the point and not a piece of a
    /// kana word: the さ of さん, the ね of ねこ. Kanji keys (各, 上, 型)
    /// are always word parts, so they pass, and so does a char a word
    /// before it ends on (高さ, 楽しみ). Otherwise a kana word starting on
    /// it rejects it, unless that word is a point too (よね for よ).
    fn stands_alone(&self, answer: &str, at: usize, vocab: &impl Vocabulary) -> bool {
        let chars: Vec<char> = answer.chars().collect();
        if !chars.get(at).copied().is_some_and(is_hiragana) {
            return true;
        }
        if (0..at).any(|start| start + vocab.word_len_at(answer, start) == at + 1) {
            return true;
        }
        let len = vocab.word_len_at(answer, at);
        let Some(word) = chars.get(at..at + len) else {
            return true;
        };
        let word: String = word.iter().collect();
        len <= 1 || !word.chars().all(is_hiragana) || !self.lookup(&word).is_empty()
    }

    /// The points found in `answer` as the hover popup finds them, two chars
    /// or more, in order. A match lying within a longer one is left out
    /// (よかった within てよかった).
    fn points_in(&self, answer: &str) -> Vec<GrammarEntry> {
        let mut matches: Vec<(usize, usize, Vec<GrammarEntry>)> = answer
            .char_indices()
            .enumerate()
            .filter_map(|(start, (i, _))| {
                let (entries, len) =
                    self.longest_match_where(answer.get(i..)?, MAX_PATTERN_CHARS, |e, chars| {
                        chars >= 2 && e.pattern.chars().count() >= 2
                    })?;
                Some((start, len, entries))
            })
            .collect();
        matches.sort_by_key(|&(start, len, _)| (std::cmp::Reverse(len), start));
        let mut kept: Vec<(usize, usize, Vec<GrammarEntry>)> = Vec::new();
        for (start, len, entries) in matches {
            let within = kept
                .iter()
                .any(|&(s, l, _)| s <= start && start + len <= s + l);
            if !within {
                kept.push((start, len, entries));
            }
        }
        kept.sort_by_key(|&(start, _, _)| start);
        let mut out: Vec<GrammarEntry> = Vec::new();
        for e in kept.into_iter().flat_map(|(_, _, entries)| entries) {
            if !out.iter().any(|o| o.id == e.id) {
                out.push(e);
            }
        }
        out
    }
}

/// The answer without spaces (a typed sentence often has some).
fn normalize(answer: &str) -> String {
    answer.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The answer with the polite negative ありません read as ない, and the
/// polite copula as the plain one, so ても差し支えありません counts for
/// ても差し支えない and 当然です for 当然だ. Patterns are written plain;
/// their polite forms are only sometimes listed. (A polite verb is
/// [`piece_at`]'s.)
fn plain_register(answer: &str) -> String {
    answer
        .replace("ありませんでした", "なかった")
        .replace("ありません", "ない")
        .replace("でしょう", "だろう")
        .replace("でした", "だった")
        .replace("です", "だ")
}

fn is_hiragana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ー')
}

/// A dictionary-form ending: the u-row kana a leading る stands for.
fn is_u_row(c: char) -> bool {
    "うくぐすつぬぶむる".contains(c)
}

/// The kana before the う of a godan volitional (行こう, 帰ろう), which a
/// leading よう stands for too (ようと思う in 帰ろうと思う).
fn is_o_row(c: char) -> bool {
    "おこごそとのぼもろ".contains(c)
}

/// Where `piece` ends when it is found at `start` of `chars`, as written or
/// conjugated (てもいい in てもいいですか, てしまう in てしまった), or in
/// the polite form of its own conjugation (考えられません for
/// 考えられない, 思いませんでした for 思わなかった).
///
/// Its start also stands for the variants of the verb before it:
/// - a leading る for any dictionary form (ることがある in 書くことがある);
/// - a leading よう for the godan volitional (ようと思う in 帰ろうと思う);
/// - a leading て or た for で or だ (た拍子に in 転んだ拍子に).
fn piece_at(piece: &str, chars: &[char], start: usize) -> Option<usize> {
    let before = start.checked_sub(1).and_then(|i| chars.get(i).copied());
    let mut voiced = String::new();
    let mut targets: Vec<&str> = vec![piece];
    if let Some(rest) = piece.strip_prefix('る')
        && !rest.is_empty()
        && before.is_some_and(is_u_row)
    {
        targets.push(rest);
    }
    if let Some(rest) = piece.strip_prefix('よ')
        && rest.starts_with('う')
        && before.is_some_and(is_o_row)
    {
        targets.push(rest);
    }
    if let Some(rest) = piece.strip_prefix('て') {
        voiced = format!("で{rest}");
    } else if let Some(rest) = piece.strip_prefix('た') {
        voiced = format!("だ{rest}");
    }
    if !voiced.is_empty() {
        targets.push(&voiced);
    }
    // A lone ない (とても〜ない) has no stem to conjugate.
    match piece {
        "ない" => targets.push("ません"),
        "なかった" => targets.push("ませんでした"),
        _ => {}
    }
    let mut deinflector = deinflect::Deinflector::default();
    // The piece's own conjugation, and its variants': 考えられない is
    // 考えられる + negative.
    let mut own: Vec<(String, Reasons)> = Vec::new();
    for t in &targets {
        own.extend(
            deinflector
                .run_with_reasons(t)
                .skip(1)
                .filter(|(_, r)| !r.is_empty())
                .map(|(f, r)| (f.to_owned(), r)),
        );
    }
    let polite = Reasons::of(&[Reason::Polite]);
    let mut longest = None;
    for end in start + 1..=chars.len() {
        let Some(sub) = chars.get(start..end) else {
            break;
        };
        let sub: String = sub.iter().collect();
        if targets.contains(&sub.as_str()) {
            return Some(end);
        }
        // A conjugated piece is at least as long as the piece minus its
        // ending, and not much longer than it.
        let piece_chars = piece.chars().count();
        if end - start + 1 < piece_chars || end - start > piece_chars + 8 {
            continue;
        }
        let same = deinflector.run_with_reasons(&sub).skip(1).any(|(f, r)| {
            targets.contains(&f)
                || own
                    .iter()
                    .any(|(of, or)| of == f && (r == *or || r == or.union(polite)))
        });
        if same {
            longest = Some(end);
        }
    }
    longest
}

/// Whether a verb of `answer` is in the form `wanted`: some stretch of it
/// deinflects through that form to a word `vocab` knows as a verb.
fn has_inflection(answer: &str, wanted: Inflection, vocab: &impl Vocabulary) -> bool {
    let need = wanted.reasons();
    let chars: Vec<char> = answer.chars().collect();
    let mut deinflector = deinflect::Deinflector::default();
    for start in 0..chars.len() {
        for end in start + 2..=chars.len().min(start + MAX_VERB_CHARS) {
            let Some(sub) = chars.get(start..end) else {
                break;
            };
            let sub: String = sub.iter().collect();
            let hit = deinflector
                .run_with_reasons(&sub)
                .skip(1)
                .any(|(base, reasons)| reasons.contains_all(need) && vocab.is_verb(base));
            if hit {
                return true;
            }
        }
    }
    false
}
