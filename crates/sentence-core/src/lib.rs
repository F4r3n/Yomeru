//! A sentence's breakdown: its words, read by jmdict-core's lattice
//! ([`jmdict_core::segment`]), and the grammar points of `grammar.toml`
//! they make up.
//!
//! Grammar is matched on the words, not on the raw text, so a pattern
//! can't start or end in the middle of a word (ら in 彼ら isn't たち's ら).
//! Three kinds of points are found:
//!
//! - **Text** patterns (てもいい, がある, なければならない), as written or
//!   conjugated. A match starts and ends where words do, or inside a
//!   word's conjugated ending: the て of 食べても.
//! - Patterns with **slots**, `〜` in `grammar.toml` (しか〜ない,
//!   たとえ〜ても): each piece on word edges, in order, with one to
//!   [`MAX_SLOT_WORDS`] words between, within a sentence.
//! - **Conjugations** (passive, causative): read from the words'
//!   deinflection, for the points whose answer check names a form.

use grammar_core::{AnswerCheck, GrammarDict, GrammarEntry, Inflection};
use jmdict_core::segment::{self, Word};

/// The longest text pattern tried, in chars.
const MAX_PATTERN_CHARS: usize = 24;

/// The most words a slot holds.
pub const MAX_SLOT_WORDS: usize = 6;

/// How a grammar point was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Text,
    Slots,
    Conjugation,
}

/// One grammar point found in a sentence.
#[derive(Debug, Clone)]
pub struct GrammarMatch {
    /// Position and length in chars, from the first piece to the last.
    pub start: usize,
    pub len: usize,
    /// The points spelled this way (homographs share a match).
    pub entries: Vec<GrammarEntry>,
    /// The pattern's own text as `(start, len)` pieces: ければ and ほど of
    /// 早ければ早いほど. One piece, the whole match, but for slots.
    pub pieces: Vec<(usize, usize)>,
    pub kind: MatchKind,
}

/// A sentence read into words and grammar.
pub struct Breakdown {
    pub words: Vec<Word>,
    /// Ordered by start, longer first.
    pub grammar: Vec<GrammarMatch>,
}

/// The grammar dictionary with what the matching needs indexed: its slot
/// patterns and its conjugation points. Build once, analyze many.
pub struct Analyzer<'a> {
    dict: &'a GrammarDict,
    /// The pieces of each key with a `〜`, and the entries it spells.
    slots: Vec<(Vec<String>, Vec<GrammarEntry>)>,
    inflections: Vec<(Inflection, GrammarEntry)>,
}

impl<'a> Analyzer<'a> {
    pub fn new(dict: &'a GrammarDict) -> Self {
        let mut slots: Vec<(String, Vec<String>, Vec<GrammarEntry>)> = Vec::new();
        let mut inflections = Vec::new();
        for e in dict.entries() {
            if let AnswerCheck::Inflection { forms } = &e.check {
                inflections.extend(forms.iter().map(|&f| (f, e.clone())));
            }
            for key in e.keys() {
                // お〜 / ご〜 take a verb stem or a noun, which the
                // dictionary's own honorific match checks.
                if !key.contains('〜') || key.starts_with("お〜") || key.starts_with("ご〜") {
                    continue;
                }
                match slots.iter_mut().find(|(k, _, _)| k == key) {
                    Some((_, _, entries)) => {
                        if !entries.iter().any(|x| x.id == e.id) {
                            entries.push(e.clone());
                        }
                    }
                    None => {
                        let pieces = key
                            .split('〜')
                            .filter(|p| !p.is_empty())
                            .map(str::to_owned)
                            .collect();
                        slots.push((key.to_owned(), pieces, vec![e.clone()]));
                    }
                }
            }
        }
        Self {
            dict,
            slots: slots.into_iter().map(|(_, p, e)| (p, e)).collect(),
            inflections,
        }
    }

    /// The words of `sentence` and the grammar they make up.
    pub fn analyze(&self, sentence: &str) -> Breakdown {
        let words = segment::segment(sentence);
        let text = Text::new(sentence, &words);
        let mut grammar = self.text_matches(&text);
        // A conjugation the text matching already found (させられた as
        // causative-passive's pattern) isn't shown twice.
        let mut conjugations = self.conjugation_matches(&words);
        conjugations.retain(|c| {
            !grammar.iter().any(|m| {
                c.start < m.start + m.len
                    && m.start < c.start + c.len
                    && c.entries
                        .iter()
                        .any(|e| m.entries.iter().any(|x| x.id == e.id))
            })
        });
        grammar.extend(conjugations);
        grammar.extend(self.slot_matches(&text));
        grammar.sort_by_key(|m| (m.start, std::cmp::Reverse(m.len)));
        Breakdown { words, grammar }
    }

    /// The longest text pattern at each point that ends where it may,
    /// less those inside a longer one.
    fn text_matches(&self, text: &Text<'_>) -> Vec<GrammarMatch> {
        let mut found: Vec<GrammarMatch> = Vec::new();
        for start in 0..text.len() {
            let soft_start = !text.can_start(start);
            if soft_start && !text.soft_start(start) {
                continue;
            }
            let Some(rest) = text.from(start) else {
                continue;
            };
            // Inside a compound (泳ぎ|方, 乗り|込んだ) a pattern may start
            // or end, but not both: it is the compound's head or its tail.
            let best = self
                .dict
                .matches_at(rest, MAX_PATTERN_CHARS)
                .into_iter()
                .find(|&(_, len)| {
                    // One char inside a word is a kana of its ending or of
                    // a compound (な of 買わなかった), not a particle,
                    // unless it ends the word (ば of 急げば).
                    (len >= 2 || text.word_start(start) || text.word_end(start + len))
                        && (text.can_end(start, start + len)
                            || (!soft_start && text.soft_end(start, start + len)))
                });
            if let Some((entries, len)) = best {
                found.push(GrammarMatch {
                    start,
                    len,
                    entries,
                    pieces: vec![(start, len)],
                    kind: MatchKind::Text,
                });
            }
        }
        // てもいい holds the particle も; only the pattern is worth showing.
        let spans: Vec<(usize, usize)> = found.iter().map(|m| (m.start, m.len)).collect();
        found.retain(|m| {
            !spans.iter().any(|&(s, l)| {
                (s, l) != (m.start, m.len) && s <= m.start && m.start + m.len <= s + l
            })
        });
        found
    }

    fn slot_matches(&self, text: &Text<'_>) -> Vec<GrammarMatch> {
        let mut out = Vec::new();
        for (pieces, entries) in &self.slots {
            for start in 0..text.len() {
                if !text.can_start(start) {
                    continue;
                }
                if let Some(spans) = text.slots_at(start, pieces)
                    && let (Some(&first), Some(&last)) = (spans.first(), spans.last())
                {
                    out.push(GrammarMatch {
                        start: first.0,
                        len: last.0 + last.1 - first.0,
                        entries: entries.clone(),
                        pieces: spans,
                        kind: MatchKind::Slots,
                    });
                }
            }
        }
        out
    }

    /// The conjugation points of each conjugated word, on its ending. A
    /// causative-passive word shows that point, not causative and passive
    /// besides.
    fn conjugation_matches(&self, words: &[Word]) -> Vec<GrammarMatch> {
        let mut out = Vec::new();
        for w in words {
            if w.reasons.is_empty() || w.entry.is_none() {
                continue;
            }
            let fits = |f: Inflection| w.reasons.contains_all(f.reasons());
            let mut entries: Vec<GrammarEntry> = Vec::new();
            for (f, e) in &self.inflections {
                let covered = Inflection::ALL
                    .iter()
                    .any(|&g| g != *f && fits(g) && g.reasons().contains_all(f.reasons()));
                if fits(*f) && !covered && !entries.iter().any(|x| x.id == e.id) {
                    entries.push(e.clone());
                }
            }
            if entries.is_empty() {
                continue;
            }
            let start = w.start + w.stem;
            let len = w.start + w.len - start;
            out.push(GrammarMatch {
                start,
                len,
                entries,
                pieces: vec![(start, len)],
                kind: MatchKind::Conjugation,
            });
        }
        out
    }
}

/// The shortest word part [`soft_edges`] splits off, in chars.
const MIN_COMPOUND_CHARS: usize = 3;

/// [`Text::soft`]: where a word of [`MIN_COMPOUND_CHARS`] or more splits
/// into a dictionary word, as written or conjugated (乗り → 乗る), and text
/// a dictionary word starts. Only within the word's stem, before its
/// conjugated ending.
fn soft_edges(chars: &[char], words: &[Word], n: usize) -> Vec<bool> {
    let mut soft = vec![false; n + 1];
    for w in words
        .iter()
        .filter(|w| w.entry.is_some() && w.len >= MIN_COMPOUND_CHARS)
    {
        let Some(text) = chars.get(w.start..w.start + w.len) else {
            continue;
        };
        for cut in 1..w.stem.min(w.len) {
            let (Some(head), Some(tail)) = (text.get(..cut), text.get(cut..)) else {
                continue;
            };
            // A kana word cut anywhere reads as words too (あ|なた,
            // いく|ら): the head has a kanji, or two chars, or is する's
            // stem (す|べき), and the tail two chars or a kanji (一年|中).
            let head_ok = head.iter().any(|&c| japanese_utils::is_kanji(c))
                || head.len() >= 2
                || matches!(head, ['す' | 'し' | 'せ']);
            let tail_ok = tail.len() >= 2 || tail.iter().any(|&c| japanese_utils::is_kanji(c));
            if !head_ok || !tail_ok {
                continue;
            }
            let head: String = head.iter().collect();
            let tail: String = tail.iter().collect();
            let whole_head = jmdict_core::lookup_longest_match(&head, MAX_PATTERN_CHARS)
                .is_some_and(|(_, len)| len == cut)
                || masu_stem_of(&head)
                    .iter()
                    .any(|v| !jmdict_core::lookup(v).is_empty());
            if whole_head
                && jmdict_core::lookup_longest_match(&tail, MAX_PATTERN_CHARS).is_some()
                && let Some(slot) = soft.get_mut(w.start + cut)
            {
                *slot = true;
            }
        }
    }
    soft
}

/// The verbs `stem` is the ます-stem of, which the deinflection rules
/// don't read: 信じ (信じる), 歌い (歌う), 乗り (乗る).
fn masu_stem_of(stem: &str) -> Vec<String> {
    let mut out = vec![format!("{stem}る")];
    if let Some((i, last)) = stem.char_indices().next_back() {
        let u = match last {
            'い' => 'う',
            'き' => 'く',
            'ぎ' => 'ぐ',
            'し' => 'す',
            'ち' => 'つ',
            'に' => 'ぬ',
            'び' => 'ぶ',
            'み' => 'む',
            'り' => 'る',
            _ => return out,
        };
        out.push(format!("{}{u}", &stem[..i]));
    }
    out
}

/// A sentence with where its words let grammar start and end.
struct Text<'s> {
    sentence: &'s str,
    chars: Vec<char>,
    /// Byte offset of each char, and of the end.
    bytes: Vec<usize>,
    /// `edge[p]`: a pattern may end at char `p`. True between words,
    /// false inside one, but true again inside a conjugated word's ending
    /// (食べ|て|も…).
    edge: Vec<bool>,
    /// `starts[p]`: a pattern may start at char `p`: between words, and
    /// within a conjugated word's ending (行か|なければ, 来れ|ば).
    starts: Vec<bool>,
    /// `word_start[p]`: a word starts at char `p`.
    word_start: Vec<bool>,
    /// `word_end[p]`: a word ends at char `p`.
    word_end: Vec<bool>,
    /// `in_word[p]`: char `p` belongs to a word.
    in_word: Vec<bool>,
    /// `soft[p]`: char `p` is inside a word that is a compound there,
    /// both sides dictionary words (泳ぎ|方, 科学|的, 乗り|込んだ): a
    /// pattern for a compound's part (方, 的, 込む) may start or end
    /// there.
    soft: Vec<bool>,
    /// Where each word starts, in order.
    word_starts: Vec<usize>,
}

impl<'s> Text<'s> {
    fn new(sentence: &'s str, words: &[Word]) -> Self {
        let chars: Vec<char> = sentence.chars().collect();
        let n = chars.len();
        let bytes: Vec<usize> = sentence
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(sentence.len()))
            .collect();
        let mut edge = vec![true; n + 1];
        let mut starts = vec![true; n + 1];
        let mut in_word = vec![false; n];
        for w in words {
            let end = w.start + w.len;
            for p in w.start..end {
                if let Some(slot) = in_word.get_mut(p) {
                    *slot = true;
                }
                if p > w.start {
                    for v in [&mut edge, &mut starts] {
                        if let Some(slot) = v.get_mut(p) {
                            *slot = false;
                        }
                    }
                }
            }
            let ending = w.start + w.attach;
            for p in ending..end {
                if let Some(slot) = edge.get_mut(p) {
                    *slot = true;
                }
            }
            for p in ending..end {
                if let Some(slot) = starts.get_mut(p) {
                    *slot = true;
                }
            }
        }
        let soft = soft_edges(&chars, words, n);
        Self {
            soft,
            sentence,
            chars,
            bytes,
            edge,
            starts,
            word_end: {
                let mut v = vec![false; n + 1];
                for w in words {
                    if let Some(slot) = v.get_mut(w.start + w.len) {
                        *slot = true;
                    }
                }
                v
            },
            word_start: {
                let mut v = vec![false; n + 1];
                for w in words {
                    if let Some(slot) = v.get_mut(w.start) {
                        *slot = true;
                    }
                }
                v
            },
            in_word,
            word_starts: words.iter().map(|w| w.start).collect(),
        }
    }

    fn len(&self) -> usize {
        self.chars.len()
    }

    fn edge(&self, p: usize) -> bool {
        self.edge.get(p).copied().unwrap_or(false)
    }

    fn in_word(&self, p: usize) -> bool {
        self.in_word.get(p).copied().unwrap_or(false)
    }

    fn soft(&self, p: usize) -> bool {
        self.soft.get(p).copied().unwrap_or(false)
    }

    fn soft_start(&self, p: usize) -> bool {
        self.soft(p) && self.in_word(p)
    }

    /// [`Self::can_end`], at a compound's inside.
    fn soft_end(&self, start: usize, end: usize) -> bool {
        end > start && self.soft(end) && (start..end).all(|p| self.in_word(p))
    }

    /// Whether a word ends at `p`.
    fn word_end(&self, p: usize) -> bool {
        self.word_end.get(p).copied().unwrap_or(false)
    }

    /// Whether a word starts at `p` (not a conjugated ending inside one).
    fn word_start(&self, p: usize) -> bool {
        self.word_start.get(p).copied().unwrap_or(false)
    }

    fn can_start(&self, p: usize) -> bool {
        self.starts.get(p).copied().unwrap_or(false) && self.in_word(p)
    }

    /// Whether a pattern from `start` may end at `end`: on an edge, and
    /// all of it within words (no punctuation inside).
    fn can_end(&self, start: usize, end: usize) -> bool {
        end > start && end <= self.len() && self.edge(end) && (start..end).all(|p| self.in_word(p))
    }

    fn from(&self, p: usize) -> Option<&'s str> {
        self.sentence.get(*self.bytes.get(p)?..)
    }

    fn slice(&self, start: usize, end: usize) -> Option<&'s str> {
        self.sentence
            .get(*self.bytes.get(start)?..*self.bytes.get(end)?)
    }

    /// Ends a sentence: a slot doesn't reach past it.
    fn stop(&self, p: usize) -> bool {
        matches!(
            self.chars.get(p),
            Some('。' | '！' | '？' | '!' | '?' | '\n')
        )
    }

    /// Words starting in `start..end`.
    fn words_in(&self, start: usize, end: usize) -> usize {
        self.word_starts
            .iter()
            .filter(|&&s| start <= s && s < end)
            .count()
    }

    /// Whether `piece` is written at `p`, ending on an edge; the end.
    fn literal_at(&self, p: usize, piece: &str) -> Option<usize> {
        let end = p + piece.chars().count();
        (self.slice(p, end)? == piece && self.can_end(p, end)).then_some(end)
    }

    /// Whether `piece` is at `p` as written or conjugated (しか〜ない with
    /// なかった), ending on an edge; the end.
    fn conjugated_at(&self, p: usize, piece: &str) -> Option<usize> {
        if let Some(end) = self.literal_at(p, piece) {
            return Some(end);
        }
        // A conjugation rewrites the ending only: the text starts like the
        // piece less its last char.
        let head: String = {
            let mut c: Vec<char> = piece.chars().collect();
            c.pop();
            c.into_iter().collect()
        };
        let rest = self.from(p)?;
        if !rest.starts_with(&head) {
            return None;
        }
        let mut deinflector = deinflect::Deinflector::default();
        let longest = (self.len() - p).min(MAX_PATTERN_CHARS);
        (1..=longest).rev().find_map(|n| {
            let end = p + n;
            if !self.can_end(p, end) {
                return None;
            }
            let surface = self.slice(p, end)?;
            deinflector.run(surface).any(|f| f == piece).then_some(end)
        })
    }

    /// The pieces of a slot pattern found from `start`, each as
    /// `(start, len)`: the first at `start`, each next one the nearest
    /// after a slot of one to [`MAX_SLOT_WORDS`] words. The last piece may
    /// be conjugated.
    fn slots_at(&self, start: usize, pieces: &[String]) -> Option<Vec<(usize, usize)>> {
        let (first, rest) = pieces.split_first()?;
        let first_end = if rest.is_empty() {
            self.conjugated_at(start, first)?
        } else {
            self.literal_at(start, first)?
        };
        let mut spans = vec![(start, first_end - start)];
        let mut at = first_end;
        for (i, piece) in rest.iter().enumerate() {
            let last = i + 1 == rest.len();
            let mut found = None;
            for p in at + 1..self.len() {
                if self.stop(p - 1) || self.words_in(at, p) > MAX_SLOT_WORDS {
                    break;
                }
                if !self.can_start(p) {
                    continue;
                }
                let end = if last {
                    self.conjugated_at(p, piece)
                } else {
                    self.literal_at(p, piece)
                };
                if let Some(end) = end {
                    found = Some((p, end));
                    break;
                }
            }
            let (p, end) = found?;
            spans.push((p, end - p));
            at = end;
        }
        Some(spans)
    }
}

#[cfg(test)]
mod tests;
