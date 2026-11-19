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

use deinflect::{Reason, Reasons};
use grammar_core::{AnswerCheck, GrammarDict, GrammarEntry, Inflection};
use jmdict_core::segment::{self, Class, Word};

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

/// The names ([`Reason::name`]) of `reasons`, in the order of
/// [`Reason::ALL`]: what a client shows of how a word is conjugated.
pub fn reason_names(reasons: Reasons) -> Vec<&'static str> {
    reasons.iter().map(Reason::name).collect()
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
    /// The points read off a word's form (`reads`): the く adverb, the さ
    /// noun.
    reads: Vec<(Reasons, GrammarEntry)>,
}

impl<'a> Analyzer<'a> {
    pub fn new(dict: &'a GrammarDict) -> Self {
        let mut slots: Vec<(String, Vec<String>, Vec<GrammarEntry>)> = Vec::new();
        let mut inflections = Vec::new();
        let mut reads = Vec::new();
        for e in dict.entries() {
            let r = e.read_reasons();
            if !r.is_empty() {
                reads.push((r, e.clone()));
            }
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
            reads,
        }
    }

    /// The words of `sentence` and the grammar they make up.
    pub fn analyze(&self, sentence: &str) -> Breakdown {
        let words = segment::segment(sentence);
        let text = Text::new(sentence, &words);
        let mut grammar = self.text_matches(&text);
        // A point that names what it comes after isn't found elsewhere.
        for m in &mut grammar {
            let head = head_before(&text.chars, &words, m.start);
            let matched: String = text.chars.iter().skip(m.start).take(m.len).collect();
            m.entries.retain(|e| {
                if e.after.is_empty() {
                    return true;
                }
                // Or a form holding its own verb stem: あり of ありえない.
                e.after.iter().any(|k| head.is(k))
                    || (e.after.iter().any(|k| k == "masu-stem")
                        && own_heads(&matched, e).any(|lead| Head::Text(lead).is("masu-stem")))
            });
        }
        grammar.retain(|m| !m.entries.is_empty());
        // A verb form inside a conjugated word is what the word's
        // conjugation says, not what its text spells: 開催された is
        // passive, though れた also spells potential's ending.
        for m in &mut grammar {
            let in_conjugated = words.iter().any(|w| {
                w.entry.is_some()
                    && !w.reasons.is_empty()
                    && w.start <= m.start
                    && m.start + m.len <= w.start + w.len
            });
            if in_conjugated {
                m.entries
                    .retain(|e| !matches!(e.check, AnswerCheck::Inflection { .. }));
            }
        }
        grammar.retain(|m| !m.entries.is_empty());
        // A conjugation the text matching already found (させられた as
        // causative-passive's pattern) isn't shown twice.
        let mut conjugations = self.conjugation_matches(&text.chars, &words);
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
        // A point read off a word's form is found that way only: its
        // pattern (く) also matches as a verb's ending (負けた as 負く).
        for m in &mut grammar {
            m.entries.retain(|e| e.reads.is_empty());
        }
        grammar.retain(|m| !m.entries.is_empty());
        grammar.extend(self.read_matches(&words));
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
    fn conjugation_matches(&self, chars: &[char], words: &[Word]) -> Vec<GrammarMatch> {
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
            // The ending, with the verb's last kana when the form changes it
            // (泳|げます, 飲|もう), without it when it is the a-row kana
            // the verb keeps for other endings too (盗ま|れた, 書か|せた).
            let a_row = chars
                .get(w.start + w.stem)
                .is_some_and(|&c| "あかがさざただなばぱまらわ".contains(c));
            let start =
                w.start + if a_row { w.attach } else { w.stem }.min(w.len.saturating_sub(1));
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

    /// The points read off a word's form ([`reads_form`]), on the whole
    /// word.
    fn read_matches(&self, words: &[Word]) -> Vec<GrammarMatch> {
        let mut out: Vec<GrammarMatch> = Vec::new();
        for (i, w) in words.iter().enumerate() {
            for (r, e) in &self.reads {
                let Some(len) = reads_form(words, i, *r) else {
                    continue;
                };
                match out.iter_mut().find(|m| {
                    (m.start, m.len) == (w.start, len) && m.kind == MatchKind::Conjugation
                }) {
                    Some(m) if !m.entries.iter().any(|x| x.id == e.id) => m.entries.push(e.clone()),
                    Some(_) => {}
                    None => out.push(GrammarMatch {
                        start: w.start,
                        len,
                        entries: vec![e.clone()],
                        pieces: vec![(w.start, len)],
                        kind: MatchKind::Conjugation,
                    }),
                }
            }
        }
        out
    }
}

/// Whether word `i` shows the form `reasons` stand for, and if so how many
/// chars the form covers from the word's start: the whole word (速く,
/// 高さ), or the word and the に after it (静か|に, the な adjective's
/// adverb). Besides the reasons the lattice gave the word, a word the
/// dictionary has whole (速く, 高さ, 便利さ) counts by its head: an い
/// adjective less its い, or for さ a な adjective.
fn reads_form(words: &[Word], i: usize, reasons: Reasons) -> Option<usize> {
    let w = words.get(i)?;
    w.entry?;
    if w.reasons.contains_all(reasons) {
        return Some(w.len);
    }
    let head_is = |head: &str, ending: &str| -> bool {
        !head.is_empty()
            && (jmdict_core::lookup(&format!("{head}い"))
                .iter()
                .any(|e| segment::classes(e).contains(&Class::Adjective))
                || (ending == "さ"
                    && jmdict_core::lookup(head)
                        .iter()
                        .any(|e| segment::classes(e).contains(&Class::NaAdjective))))
    };
    let only = |r: Reason| reasons == Reasons::of(&[r]);
    if only(Reason::Adverbial) {
        if w.reasons.is_empty()
            && let Some(head) = w.base.strip_suffix('く')
            && head_is(head, "く")
        {
            return Some(w.len);
        }
        let next = words.get(i + 1)?;
        let na = w.reasons.is_empty()
            && w.entry
                .is_some_and(|e| segment::classes(e).contains(&Class::NaAdjective))
            && next.start == w.start + w.len
            && next.class == Class::Particle
            && next.base == "に";
        return na.then_some(w.len + next.len);
    }
    if only(Reason::Nominalization) && w.reasons.is_empty() {
        let head = w.base.strip_suffix('さ')?;
        return head_is(head, "さ").then_some(w.len);
    }
    None
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
        // A verb or adjective splits after a verb's ます-stem or an
        // adjective's stem (立ち|上がる, 怖|がる, 古|びた), or a head of two
        // chars (大人|びる), not after any word (異なる isn't 異 + なる).
        let one_piece = matches!(w.class, Class::Verb | Class::Adjective)
            && !w
                .entry
                .is_some_and(|e| segment::classes(e).contains(&Class::Expression));
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
            // A suffix or auxiliary takes any head (女|らしい, 春|めく), if no
            // word of its own is spelled the same (なる, 成る).
            let suffix = || {
                jmdict_core::lookup_longest_match(&tail, MAX_PATTERN_CHARS).is_some_and(
                    |(es, _)| {
                        let affix = |e: &&_| {
                            let c = segment::classes(e);
                            c.contains(&Class::Suffix) || c.contains(&Class::Auxiliary)
                        };
                        es.iter().any(affix)
                            && !es.iter().any(|e| {
                                let c = segment::classes(e);
                                !affix(e)
                                    && (c.contains(&Class::Verb) || c.contains(&Class::Adjective))
                            })
                    },
                )
            };
            if one_piece
                && cut < 2
                && !suffix()
                && segment::masu_stem_verbs(&head).is_empty()
                && !jmdict_core::lookup(&format!("{head}い"))
                    .iter()
                    .any(|e| segment::classes(e).contains(&Class::Adjective))
            {
                continue;
            }
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

/// What comes before a match: the word before it, with its text, or, for a
/// match starting inside a word (釣り|に行く of 釣りに行く), the text of
/// that word before it.
enum Head<'w> {
    /// The word, its text, whether a numeral comes right before it (the
    /// counter of 二|リットル, ５００００|円), and whether it follows a verb
    /// or adjective (る of 走って|る).
    Word(&'w Word, String, bool, bool),
    Text(String),
    /// Digits, which the lattice leaves between words (１００は).
    Digits,
    Nothing,
}

fn head_before<'w>(chars: &[char], words: &'w [Word], at: usize) -> Head<'w> {
    let gap_digits = |from: usize| {
        let gap = chars.get(from..at).unwrap_or_default();
        !gap.is_empty() && gap.iter().all(|&c| is_numeral(c))
    };
    let Some(w) = words.iter().rfind(|w| w.start < at) else {
        return if gap_digits(0) {
            Head::Digits
        } else {
            Head::Nothing
        };
    };
    let text = |end: usize| chars.get(w.start..end).unwrap_or_default().iter().collect();
    match (w.start + w.len).cmp(&at) {
        std::cmp::Ordering::Equal => {
            let numeral_before = w
                .start
                .checked_sub(1)
                .and_then(|i| chars.get(i))
                .is_some_and(|&c| is_numeral(c));
            let after_verb = words
                .iter()
                .rfind(|p| p.start + p.len == w.start)
                .is_some_and(|p| {
                    matches!(p.class, Class::Verb | Class::Adjective | Class::Auxiliary)
                });
            Head::Word(w, text(at), numeral_before, after_verb)
        }
        // A gap between it and the match: digits, or punctuation.
        std::cmp::Ordering::Less if gap_digits(w.start + w.len) => Head::Digits,
        std::cmp::Ordering::Less => Head::Nothing,
        std::cmp::Ordering::Greater => Head::Text(text(at)),
    }
}

/// The leads a matched form of `e` may hold its own verb stem in: あり,
/// ありえ… of ありえない (the uru point written whole). Not the start of
/// the point's own text: 得 of 得よう, に of に行く.
fn own_heads<'m>(matched: &'m str, e: &'m GrammarEntry) -> impl Iterator<Item = String> + 'm {
    let first = e.pattern.chars().next();
    matched
        .char_indices()
        .skip(1)
        .map(|(i, _)| matched[..i].to_owned())
        .filter(move |lead| {
            !e.keys()
                .filter(|k| k.chars().next() == first)
                .any(|k| k.starts_with(lead.as_str()))
        })
}

/// A digit or a kanji numeral: 3, ３, 三, 十, 万.
fn is_numeral(c: char) -> bool {
    c.is_ascii_digit()
        || ('０'..='９').contains(&c)
        || "〇一二三四五六七八九十百千万億兆".contains(c)
}

impl Head<'_> {
    /// Whether this is a word of `kind` ([`grammar_core::AFTER_KINDS`]).
    fn is(&self, kind: &str) -> bool {
        let spelled = |e: &jmdict_core::segment::Entry, t: &str| {
            e.kanji_forms.iter().any(|k| k.text.as_str() == t)
                || e.reading_forms.iter().any(|r| r.text.as_str() == t)
        };
        match (self, kind) {
            // An amount: a word that starts with a numeral (三十分, 一度,
            // 五つ), a word right after one (二|リットル), or digits.
            (Head::Word(_, t, numeral_before, _), "amount") => {
                *numeral_before || t.chars().next().is_some_and(is_numeral)
            }
            (Head::Text(t), "amount") => t.chars().next().is_some_and(is_numeral),
            (Head::Digits, "amount") => true,
            // A noun (東京のような, 雪のように), a name in katakana, or
            // この, その, あの, どの (このような).
            (Head::Word(w, _, _, _), "noun-or-demonstrative") => {
                matches!(w.class, Class::Noun | Class::Suffix | Class::Unknown)
                    || (w.class == Class::Prenominal && w.base.ends_with('の'))
            }
            (Head::Digits, "noun-or-demonstrative" | "noun-or-particle") => true,
            // A noun (私の), a suffix (由利さんの), a name in katakana, or
            // a particle (東京からの): not a verb or adjective, whose の
            // explains or asks (得たのか).
            (Head::Word(w, _, _, after_verb), "noun-or-particle") => {
                // Unconjugated (失敗した is a verb), and a suffix on a noun
                // (さん of 由利さん, not る of 走って|る).
                w.reasons.is_empty()
                    && match w.class {
                        Class::Noun | Class::Unknown | Class::Particle => true,
                        Class::Suffix => !after_verb,
                        _ => false,
                    }
            }
            (Head::Text(t), "noun-or-particle") => jmdict_core::lookup(t)
                .iter()
                .any(|e| spelled(e, t) && segment::classes(e).contains(&Class::Noun)),
            (Head::Text(t), "noun-or-demonstrative") => {
                ["こ", "そ", "あ", "ど"].contains(&t.as_str())
                    || jmdict_core::lookup(t)
                        .iter()
                        .any(|e| spelled(e, t) && segment::classes(e).contains(&Class::Noun))
            }
            (Head::Word(w, _, _, _), "verb-plain") => {
                (w.class == Class::Verb && w.reasons.is_empty())
                    || w.reasons == Reasons::of(&[Reason::SuruStem])
            }
            (Head::Word(w, t, _, _), "masu-stem") => {
                w.reasons == Reasons::of(&[Reason::MasuStem])
                    || (w.reasons.is_empty() && !segment::masu_stem_verbs(t).is_empty())
            }
            (Head::Word(w, _, _, _), "suru-noun") => {
                w.reasons.is_empty() && w.entry.is_some_and(segment::takes_suru)
            }
            (Head::Text(t), "verb-plain") => jmdict_core::lookup(t)
                .iter()
                .any(|e| segment::classes(e).contains(&Class::Verb) && spelled(e, t)),
            (Head::Text(t), "masu-stem") => !segment::masu_stem_verbs(t).is_empty(),
            (Head::Text(t), "suru-noun") => jmdict_core::lookup(t)
                .iter()
                .any(|e| segment::takes_suru(e) && spelled(e, t)),
            _ => false,
        }
    }
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
