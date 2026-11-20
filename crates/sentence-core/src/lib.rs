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

mod check;

use deinflect::{Reason, Reasons};
use grammar_core::{AnswerCheck, GrammarDict, GrammarEntry, Inflection};
use jmdict_core::segment::{self, Class, Word};
use std::cell::Cell;

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

/// Whether the verb `base` (働かせる) is the causative of another
/// dictionary verb (働く): then the chars the two share.
fn lexical_causative(deinflector: &mut deinflect::Deinflector, base: &str) -> Option<usize> {
    if !(base.ends_with("せる") || base.ends_with("させる")) {
        return None;
    }
    let root = deinflector
        .run_with_reasons(base)
        .filter(|(_, r)| *r == Reasons::of(&[Reason::Causative]))
        .map(|(f, _)| f.to_owned())
        .find(|f| {
            jmdict_core::lookup(f)
                .iter()
                .any(|e| segment::classes(e).contains(&Class::Verb))
        })?;
    Some(
        base.chars()
            .zip(root.chars())
            .take_while(|(a, b)| a == b)
            .count(),
    )
}

/// The most chars after a phrase read along when splitting it.
const MAX_PHRASE_CONTEXT: usize = 12;

/// `words` with each phrase ([`Word::phrase`]) replaced by its words, and
/// the phrases split.
fn split_phrases(chars: &[char], words: Vec<Word>) -> (Vec<Word>, Vec<Word>) {
    let mut out = Vec::with_capacity(words.len());
    let mut phrases = Vec::new();
    for w in words {
        let surface: String = chars.iter().skip(w.start).take(w.len).collect();
        // What follows, to the end of the clause.
        let after: String = chars
            .iter()
            .skip(w.start + w.len)
            .take(MAX_PHRASE_CONTEXT)
            .take_while(|&&c| !matches!(c, '、' | '。' | '，' | '．' | '！' | '？'))
            .collect();
        // A conjugated phrase keeps its conjugation on its last word: 気が
        // して isn't 気|が|して with the particle して.
        let split = w
            .phrase
            .then(|| segment::split_phrase(&surface, &after))
            .flatten()
            .filter(|parts| {
                w.reasons.is_empty() || parts.last().is_some_and(|p| p.reasons == w.reasons)
            });
        match split {
            Some(parts) => {
                out.extend(parts.into_iter().map(|p| Word {
                    start: w.start + p.start,
                    ..p
                }));
                phrases.push(w);
            }
            None => out.push(w),
        }
    }
    (out, phrases)
}

/// The names ([`Reason::name`]) of `reasons`, in the order of
/// [`Reason::ALL`]: what a client shows of how a word is conjugated.
pub fn reason_names(reasons: Reasons) -> Vec<&'static str> {
    reasons.iter().map(Reason::name).collect()
}

/// A sentence read into words and grammar.
pub struct Breakdown {
    /// A phrase JMdict lists whole (と言った, 雨が降る) shows as its
    /// words: と|言った.
    pub words: Vec<Word>,
    /// Ordered by start, longer first.
    pub grammar: Vec<GrammarMatch>,
    /// The phrases split in `words`, as the expressions read: their
    /// entries, a learner's to open.
    pub phrases: Vec<Word>,
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
        let chars: Vec<char> = sentence.chars().collect();
        // The grammar is read on the words a learner sees: a phrase JMdict
        // lists whole (家を出る) split, so its particle is a point too.
        let (words, phrases) = split_phrases(&chars, segment::segment(sentence));
        let text = Text::new(sentence, &words);
        let mut grammar = self.text_matches(&text, &words);
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
        // A point spelled as written holds its command, volitional or
        // potential form: the しろ of 雨にしろ, the ろう of てやろう, the
        // いけない of てはいけない aren't points of their own.
        conjugations.retain(|c| {
            let fixed = c.entries.iter().all(|e| {
                matches!(&e.check, AnswerCheck::Inflection { forms }
                    if forms.iter().all(|f| matches!(f, Inflection::Imperative | Inflection::Volitional | Inflection::Potential)))
            });
            !fixed || !grammar.iter().any(|m| {
                m.start <= c.start
                    && c.start + c.len <= m.start + m.len
                    && (m.start, m.len) != (c.start, c.len)
                    && {
                        let t: String = text.chars.iter().skip(m.start).take(m.len).collect();
                        self.dict
                            .lookup(&t)
                            .iter()
                            .any(|e| m.entries.iter().any(|x| x.id == e.id))
                    }
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
        let mut slots = self.slot_matches(&text);
        for m in &mut slots {
            let head = head_before(&text.chars, &words, m.start);
            m.entries
                .retain(|e| e.after.is_empty() || e.after.iter().any(|k| head.is(k)));
            // A verb (or a する noun with its する) right after を takes it
            // as its object: モックを推奨するロンドン学派とし isn't を〜とする.
            if let Some(&(s, n)) = m.pieces.first()
                && text.chars.get(s + n - 1) == Some(&'を')
                && words
                    .iter()
                    .find(|w| w.start == s + n)
                    .is_some_and(|w| w.class == Class::Verb || w.reasons.contains(Reason::SuruStem))
            {
                m.entries.clear();
            }
            // After a bare topic は, a が right after a verb or adjective
            // joins two clauses, "but": 妻はいないとしてきたが、… isn't は〜が
            // (ことは〜が's が is that "but", 行くことは行くが).
            if m.pieces
                .first()
                .is_some_and(|&(s, n)| n == 1 && text.chars.get(s) == Some(&'は'))
                && m.pieces.iter().any(|&(s, n)| {
                    n == 1
                        && text.chars.get(s) == Some(&'が')
                        // Or a copula's ending (そうだが, のですが).
                        && (head_before(&text.chars, &words, s).is("predicate")
                            || s > 0
                                && ['だ', 'す'].contains(text.chars.get(s - 1).unwrap_or(&' '))
                                && words.iter().any(|w| w.start + w.len == s && w.len >= 2))
                })
            {
                m.entries.clear();
            }
        }
        slots.retain(|m| !m.entries.is_empty());
        grammar.extend(slots);
        grammar.sort_by_key(|m| (m.start, std::cmp::Reverse(m.len)));
        Breakdown {
            words,
            grammar,
            phrases,
        }
    }

    /// The longest text pattern at each point that ends where it may,
    /// less those inside a longer one.
    fn text_matches(&self, text: &Text<'_>, words: &[Word]) -> Vec<GrammarMatch> {
        let mut found: Vec<GrammarMatch> = Vec::new();
        for start in 0..text.len() {
            let soft_start = !text.can_start(start);
            let head = head_before(&text.chars, words, start);
            // Inside a word, but not at a compound's edge: only a suffix
            // point ending the word (新|型, 痛|み).
            let inside = soft_start && !text.soft_start(start);
            // The last kana of a plain verb (食べ|る), where a point holding
            // it starts (るところだ, ることがある): the end of the verb.
            let tail_end = words
                .iter()
                .find(|w| {
                    w.start < start
                        && start + 1 == w.start + w.len
                        && w.class == Class::Verb
                        && w.reasons.is_empty()
                })
                .map(|w| w.start + w.len)
                .filter(|_| {
                    text.chars
                        .get(start)
                        .is_some_and(|&c| "うくぐすつぬぶむる".contains(c))
                });
            // A conjugated verb's sound change (持っ|て, 選ん|だ): its っ or ん
            // opens no point (not って "they say", not んだ), and its て, で,
            // た or だ alone is the form's ending (not the copula だ), read
            // only as the て-form point.
            let onbin = |p: usize| {
                text.chars.get(p).is_some_and(|c| ['っ', 'ん'].contains(c))
                    && text
                        .chars
                        .get(p + 1)
                        .is_some_and(|c| ['て', 'で', 'た', 'だ'].contains(c))
                    && words.iter().any(|w| {
                        w.start < p
                            && p + 1 < w.start + w.len
                            && ((matches!(w.class, Class::Verb | Class::Auxiliary)
                                    && !w.reasons.is_empty())
                                    // An expression in its own past (だった).
                                    || (w.class == Class::Expression
                                        && text.chars.get(w.start + w.len - 1).is_some_and(
                                            |c| ['て', 'で', 'た', 'だ'].contains(c),
                                        )))
                    })
            };
            if onbin(start) {
                continue;
            }
            let own_ending = start > 0 && onbin(start - 1);
            // In an adjective, an edge between a kanji and its kana (温|かい)
            // is where the word's kana start: only a point after an
            // adjective's stem or a suffix goes there (寒|がる), not かい.
            let okurigana = soft_start
                && words.iter().any(|w| {
                    w.start < start && start < w.start + w.len && w.class == Class::Adjective
                })
                && text.soft_start(start)
                && start > 0
                && text
                    .chars
                    .get(start - 1)
                    .is_some_and(|&c| japanese_utils::is_kanji(c))
                && text
                    .chars
                    .get(start)
                    .is_some_and(|&c| japanese_utils::is_kana(c));
            // The word starting here, for a prefix point ending inside it.
            let word_here = words.iter().find(|w| w.start == start && w.len >= 2);
            let Some(rest) = text.from(start) else {
                continue;
            };
            // Inside a compound (泳ぎ|方, 乗り|込んだ) a pattern may start
            // or end, but not both: it is the compound's head or its tail.
            // Where a match ends decides between readings: a point that
            // ends a sentence (の of 行くの？, not of 行くのは), and one
            // that never does (にしろ of 雨にしろ、; at the end it is
            // にする's command), and one whose に or と isn't にする's,
            // になる's or とする's (以上にしろ is 以上 + にする; いないとして
            // quotes, it isn't ないと).
            // One char inside a word is a kana of its ending or of a
            // compound (な of 買わなかった), not a particle, unless it ends
            // the word (ば of 急げば).
            let ends_well = |len: usize| {
                (len >= 2 || text.word_start(start) || text.word_end(start + len))
                    && (text.can_end(start, start + len)
                        || (!soft_start && text.soft_end(start, start + len)))
            };
            // A prefix point ending inside the word that starts here (各|国,
            // 真っ|暗): the rest is a word, or an adjective's stem.
            let prefix_end = |len: usize| {
                word_here.is_some_and(|w| {
                    len < w.len && {
                        let rest = text.slice(start + len, w.start + w.len).unwrap_or_default();
                        !jmdict_core::lookup(rest).is_empty()
                            || jmdict_core::lookup(&format!("{rest}い"))
                                .iter()
                                .any(|e| segment::classes(e).contains(&Class::Adjective))
                    }
                })
            };
            // The entries [`keep`] lets through at each length have been
            // checked for the affix cases.
            // From a verb's last kana: three chars or more, past the verb
            // (not うと of 言うと, るな).
            let from_tail = |len: usize| {
                tail_end.is_some_and(|end| len >= 3 && start + len > end)
                    && text.can_end(start, start + len)
            };
            let fits_text = |len: usize| {
                if inside {
                    text.word_end(start + len) || from_tail(len)
                } else {
                    ends_well(len) || prefix_end(len)
                }
            };
            // The longest length whose point was turned down only for what
            // it comes after: a shorter match there would be a piece of it
            // (の of よいのか once のか is out).
            let refused = Cell::new(0);
            // A one-kanji point that goes after a word (中, 前, 的) isn't
            // the first kanji of a longer word (中心部), unless the word is
            // an expression holding it (姿を消す).
            let kanji_inside_word = |e: &GrammarEntry, matched: &str| {
                let mut chars = matched.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return false;
                };
                let prefix = e.prefix
                    || e.formation
                        .iter()
                        .any(|f| e.keys().any(|k| f.starts_with(k)));
                japanese_utils::is_kanji(c)
                    && !prefix
                    && text.word_start(start)
                    && !text.word_end(start + 1)
                    && words.iter().find(|w| w.start == start).is_some_and(|w| {
                        w.entry
                            .is_none_or(|e| !segment::classes(e).contains(&Class::Expression))
                    })
            };
            let keep = |e: &GrammarEntry, len: usize| {
                let end = start + len;
                let before_suru = e.no_suru
                    && end > 0
                    && text
                        .chars
                        .get(end - 1)
                        .is_some_and(|c| ['に', 'と'].contains(c))
                    && words.iter().any(|w| {
                        (w.start == end
                            && ["する", "為る", "なる", "成る"].contains(&w.base.as_str()))
                            // Or the particle read with it: として (とする).
                            || (w.start + 1 == end
                                && ["にする", "になる", "とする", "となる"]
                                    .contains(&w.base.as_str()))
                    });
                // A と before a verb of saying or thinking quotes.
                let before_quote = e.no_quote
                    && end > 0
                    && text.chars.get(end - 1) == Some(&'と')
                    && words.iter().any(|w| {
                        w.start == end
                            && [
                                "言う",
                                "いう",
                                "云う",
                                "思う",
                                "おもう",
                                "考える",
                                "答える",
                                "叫ぶ",
                                "書く",
                                "聞く",
                                "信じる",
                                "感じる",
                                "述べる",
                                "話す",
                                "伝える",
                                "申す",
                                "申し上げる",
                            ]
                            .contains(&w.base.as_str())
                    });
                // A clause-ending point before a particle is the noun it
                // spells: 試験の結果が出た.
                let before_particle = e.no_particle
                    && words.iter().any(|w| {
                        w.start == end
                            && w.class == Class::Particle
                            && ["が", "を", "は", "も", "に", "の", "と", "で", "へ", "まで"]
                                .contains(&w.base.as_str())
                    });
                // An adjective's own suffix (熱っぽい, 未練がましい) goes there too.
                if okurigana && !e.suffix && !e.after.iter().any(|k| k == "adjective-stem") {
                    return false;
                }
                // A point read conjugated isn't a run of particles: なよ,
                // split な|よ, isn't なる's.
                let matched_text = text.slice(start, end).unwrap_or_default();
                let over: Vec<&Word> = words
                    .iter()
                    .filter(|w| w.start < end && start < w.start + w.len)
                    .collect();
                let conjugated = !e.keys().any(|k| k == matched_text);
                // Nor are a word's forms that don't conjugate: つく isn't the
                // adverb つい's.
                let fixed_word = |k: &str| {
                    let entries = jmdict_core::lookup(k);
                    let spelled: Vec<_> = entries
                        .iter()
                        .filter(|x| {
                            x.kanji_forms.iter().any(|f| f.text.as_str() == k)
                                || x.reading_forms.iter().any(|f| f.text.as_str() == k)
                        })
                        .collect();
                    !spelled.is_empty()
                        && spelled.iter().all(|x| {
                            !segment::classes(x).iter().any(|c| {
                                matches!(
                                    c,
                                    Class::Verb
                                        | Class::Adjective
                                        | Class::Auxiliary
                                        | Class::Expression
                                )
                            })
                        })
                };
                if conjugated && e.keys().all(fixed_word) {
                    return false;
                }
                if conjugated
                    && !over.is_empty()
                    && over
                        .iter()
                        .all(|w| w.class == Class::Particle && w.reasons.is_empty())
                {
                    return false;
                }
                // Nor is the start of a conjugated word, stopping inside it
                // (かな of かなわなかった), but for a prefix point.
                // The rest a word of its own reads as that word's split
                // (時間さえ|ない, a 冴えない misread).
                if !e.prefix
                    && words.iter().any(|w| {
                        w.start == start && end < w.start + w.len && !w.reasons.is_empty() && {
                            let rest = text.slice(end, w.start + w.len).unwrap_or_default();
                            let n = rest.chars().count();
                            !segment::candidates_at(rest, 0, &segment::Costs::default())
                                .iter()
                                // A likely one: not わなかった as 悪い's.
                                .any(|&(len, class, cost, _)| {
                                    len == n && class != Class::Unknown && cost <= 2000
                                })
                        }
                    })
                {
                    return false;
                }
                // A verb's stem before its own ます is no point (あり of
                // あります).
                if text.chars.get(end) == Some(&'ま')
                    && words.iter().any(|w| {
                        w.start < end && end < w.start + w.len && w.reasons.contains(Reason::Polite)
                    })
                {
                    return false;
                }
                if own_ending && len == 1 && e.pattern != "て" {
                    return false;
                }
                let matched = text.slice(start, end).unwrap_or_default();
                let affix = if inside {
                    (e.suffix && !e.after.is_empty() && text.word_end(end)) || from_tail(len)
                } else {
                    ends_well(len) || (e.prefix && prefix_end(len))
                };
                if !affix {
                    return false;
                }
                // 、 closes a clause the の ends, but 雨にしろ、 goes on. A
                // sentence's last word is written as is: といって、 isn't
                // the hearsay という.
                let fits = (!e.ends
                    || (ends_here(&text.chars, words, end, true)
                        && e.keys().any(|k| k == matched)))
                    && !(e.continues && ends_here(&text.chars, words, end, false))
                    && !before_suru
                    && !before_quote
                    && !before_particle
                    && !kanji_inside_word(e, matched);
                // A point that names what it comes after isn't found
                // elsewhere: then the text's other forms of that length are
                // tried (いないとして is とする's, not として "as").
                if fits && !fits_after(e, &head, matched) {
                    if fits_text(len) {
                        refused.set(refused.get().max(len));
                    }
                    return false;
                }
                fits
            };
            let best = self
                .dict
                .matches_at_where(rest, MAX_PATTERN_CHARS, keep)
                .into_iter()
                .find(|&(_, len)| fits_text(len))
                .filter(|&(_, len)| len >= refused.get());
            if let Some((mut entries, len)) = best {
                // A homograph comes along in its partner's group: だから
                // is kara-reason's spelling, not kara-from's.
                if let Some(matched) = text.slice(start, start + len)
                    && entries.iter().any(|e| e.keys().any(|k| k == matched))
                {
                    entries.retain(|e| e.keys().any(|k| k == matched));
                }
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
        /// The longest gap, in chars, between `pieces` found in order in
        /// an example's mark (何人か: 1); `None` if they aren't all there.
        fn slot_gap(mark: &str, pieces: &[String]) -> Option<usize> {
            let (first, rest) = pieces.split_first()?;
            let mut at = mark.find(first.as_str())? + first.len();
            let mut longest = 0;
            for piece in rest {
                let i = mark.get(at..)?.find(piece.as_str())?;
                longest = longest.max(mark[at..at + i].chars().count());
                at += i + piece.len();
            }
            Some(longest)
        }
        let mut out = Vec::new();
        for (pieces, entries) in &self.slots {
            // A slot crosses 、 only for a point whose examples do: paired
            // points (もあれば、…も, 書いては消し、書いては), not は〜が.
            let commas = entries.iter().any(|e| {
                e.examples.iter().any(|x| {
                    x.ja.iter().any(|s| {
                        matches!(s, grammar_core::Span::Highlight { text } if text.contains('、'))
                    })
                })
            });
            // Nor longer than about twice the longest the examples show:
            // 何〜か takes a counter (何人か), not a clause.
            let longest = entries
                .iter()
                .flat_map(|e| e.examples.iter())
                .flat_map(|x| x.ja.iter())
                .filter_map(|s| match s {
                    grammar_core::Span::Highlight { text } => slot_gap(text, pieces),
                    _ => None,
                })
                .max();
            let max_chars = longest.map_or(usize::MAX, |n| 2 * n + 2);
            for start in 0..text.len() {
                if !text.can_start(start) {
                    continue;
                }
                if let Some(spans) = text.slots_at(start, pieces, commas)
                    && spans.windows(2).all(|w| match w {
                        [a, b] => b.0.saturating_sub(a.0 + a.1) <= max_chars,
                        _ => true,
                    })
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
        let mut deinflector = deinflect::Deinflector::default();
        for w in words {
            if w.entry.is_none() {
                continue;
            }
            // A causative JMdict lists as a verb of its own (働かせる,
            // 待たせる) is still one: its own verb's, from the char after
            // what the two share (働か|せた).
            let lexical = (!w.reasons.contains(Reason::Causative))
                .then(|| lexical_causative(&mut deinflector, &w.base))
                .flatten();
            let reasons = match lexical {
                Some(_) => w.reasons.union(Reasons::of(&[Reason::Causative])),
                None => w.reasons,
            };
            if reasons.is_empty() {
                continue;
            }
            let fits = |f: Inflection| reasons.contains_all(f.reasons());
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
            // する and 来る change whole (しろ, 来い, させる).
            let whole = ["する", "為る", "くる", "来る"].contains(&w.base.as_str());
            let start = w.start
                + if whole {
                    0
                } else if a_row {
                    w.attach
                } else {
                    w.stem
                }
                .min(w.len.saturating_sub(1));
            let start = match lexical {
                Some(shared) => {
                    let after = w.start + shared;
                    let a_row = chars
                        .get(after)
                        .is_some_and(|&c| "あかがさざただなばぱまらわ".contains(c));
                    (after + usize::from(a_row)).min(w.start + w.len.saturating_sub(1))
                }
                None => start,
            };
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
    let only = |r: Reason| reasons == Reasons::of(&[r]);
    // Before なる or する the adverb form goes with it: 高くなる,
    // 得なくなる, 容易になる are "become …", not "…ly".
    // なり of 得なくなり is a word of its own, but なる's stem too.
    let becomes = |j: usize, at: usize| {
        // 見える too: 元気に見える "looks well" is に見える.
        let verb = |base: &str| ["なる", "成る", "為る", "する", "見える"].contains(&base);
        words.get(j).is_some_and(|v| {
            v.start == at
                && (verb(&v.base)
                    || segment::masu_stem_verbs(&v.base).iter().any(|e| {
                        e.kanji_forms.iter().any(|k| verb(k.text.as_str()))
                            || e.reading_forms.iter().any(|r| verb(r.text.as_str()))
                    }))
        })
    };
    if only(Reason::Adverbial) && becomes(i + 1, w.start + w.len) {
        return None;
    }
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
    if only(Reason::Adverbial) {
        // An adverb of its own (まったく, 全く) isn't an adjective's form,
        // though its head spells one (全い); one that is a noun too (多く,
        // 遠く) is.
        let adverb_only = w.entry.is_some_and(|e| {
            let c = segment::classes(e);
            c.contains(&Class::Adverb) && !c.contains(&Class::Noun)
        });
        if w.reasons.is_empty()
            && !adverb_only
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
        // Not the に of になる, にする: 容易になった is "became easy".
        return (na && !becomes(i + 2, next.start + next.len)).then_some(w.len + next.len);
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
            // す only before べき: すてる isn't す|てる.
            let head_ok = head.iter().any(|&c| japanese_utils::is_kanji(c))
                || head.len() >= 2
                || matches!(head, ['し' | 'せ'])
                || (head == ['す'] && tail.first() == Some(&'べ'));
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

/// Whether a sentence may end at `at`: the end, punctuation, a line
/// break, or a sentence-ending particle (よ, ね, さ, な, かな…) closing it.
/// 、 counts only with `comma`.
fn ends_here(chars: &[char], words: &[Word], at: usize, comma: bool) -> bool {
    let Some(&next) = chars.get(at) else {
        return true;
    };
    if next == '、' {
        return comma;
    }
    if !japanese_utils::is_japanese(next) || "。！？!?」』）…・〜".contains(next) {
        return true;
    }
    words.iter().find(|w| w.start == at).is_some_and(|w| {
        let t: String = chars.iter().skip(w.start).take(w.len).collect();
        ["よ", "ね", "さ", "な", "かな", "かしら", "よね"].contains(&t.as_str())
            && ends_here(chars, words, w.start + w.len, comma)
    })
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
                        // A noun read as an adverb (夜 of 夜型, 今日).
                        Class::Adverb => w
                            .entry
                            .is_some_and(|e| segment::classes(e).contains(&Class::Noun)),
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
            // A verb, adjective or ending, conjugated or not (行くの,
            // 行ったのか, 高いの, 走って|る|の).
            (Head::Word(w, _, _, after_verb), "predicate") => match w.class {
                Class::Verb | Class::Adjective | Class::Auxiliary => true,
                Class::Suffix => *after_verb,
                _ => !w.reasons.is_empty(),
            },
            (Head::Text(t), "predicate") => jmdict_core::lookup(t).iter().any(|e| {
                spelled(e, t)
                    && segment::classes(e)
                        .iter()
                        .any(|c| matches!(c, Class::Verb | Class::Adjective | Class::Auxiliary))
            }),
            // An adjective's stem (寒|がる, 欲し|がる), a な adjective
            // (嫌|がる), or a verb's stem + た (行きた|がる).
            (Head::Word(_, t, _, _) | Head::Text(t), "adjective-stem") => {
                let is =
                    |e: &jmdict_core::segment::Entry, c: Class| segment::classes(e).contains(&c);
                jmdict_core::lookup(&format!("{t}い"))
                    .iter()
                    .any(|e| is(e, Class::Adjective))
                    || jmdict_core::lookup(t)
                        .iter()
                        .any(|e| spelled(e, t) && is(e, Class::NaAdjective))
                    || t.strip_suffix('た')
                        .is_some_and(|stem| !segment::masu_stem_verbs(stem).is_empty())
            }
            // Not a kind of its own: a noun or な-adjective before the な of
            // a predicate (休みなの, 好きなのか).
            (Head::Word(w, _, _, _), "noun") => {
                w.reasons.is_empty()
                    && matches!(w.class, Class::Noun | Class::NaAdjective | Class::Unknown)
            }
            (Head::Word(w, _, _, _), "verb-plain") => {
                (w.class == Class::Verb && w.reasons.is_empty())
                    || w.reasons == Reasons::of(&[Reason::SuruStem])
            }
            // Or text read otherwise that is a verb's stem too: 取れ, 取る's
            // command, is 取れる's stem (取れかける).
            // 来, 来る's stem, which is irregular (来そうだ).
            (Head::Word(_, t, _, _) | Head::Text(t), "masu-stem") if t == "来" => true,
            (Head::Word(w, t, _, _), "masu-stem") => {
                w.reasons == Reasons::of(&[Reason::MasuStem])
                    // Not a particle spelling one (で, 出る's).
                    || ((w.reasons.is_empty() || w.reasons == Reasons::of(&[Reason::Imperative]))
                        && w.class != Class::Particle
                        && !segment::masu_stem_verbs(t).is_empty())
            }
            (Head::Word(w, _, _, _), "suru-noun") => {
                w.reasons.is_empty() && w.entry.is_some_and(segment::takes_suru)
            }
            (Head::Text(t), "verb-plain") => jmdict_core::lookup(t)
                .iter()
                .any(|e| segment::classes(e).contains(&Class::Verb) && spelled(e, t)),
            // A する noun's し too (休憩し|たい), or する's own.
            (Head::Text(t), "masu-stem") => {
                !segment::masu_stem_verbs(t).is_empty()
                    || t == "し"
                    || t.strip_suffix('し').is_some_and(|n| {
                        jmdict_core::lookup(n)
                            .iter()
                            .any(|e| spelled(e, n) && segment::takes_suru(e))
                    })
            }
            (Head::Text(t), "suru-noun") => jmdict_core::lookup(t)
                .iter()
                .any(|e| segment::takes_suru(e) && spelled(e, t)),
            _ => false,
        }
    }
}

/// Whether `e` may come after `head`, where the text it matched is
/// `matched`.
fn fits_after(e: &GrammarEntry, head: &Head<'_>, matched: &str) -> bool {
    e.after.is_empty()
        || e.after.iter().any(|k| head.is(k))
        // The copula a noun takes before a predicate's ending (休みなの,
        // 雨だから, 学生ですから), or opening a sentence (だから、…).
        || (e.after.iter().any(|k| k == "predicate")
            && matched.starts_with(['な', 'だ', 'で'])
            && (head.is("noun") || matches!(head, Head::Nothing)))
        // A form holding its verb: あり of ありえない, 泣く of 泣くに泣けない.
        // A key holds that verb and goes on as the text does (ありえない for
        // ありえなかった): not any stem the text opens with (抜け of
        // 抜けません for 抜く).
        || e.after.iter().any(|k| {
            ["masu-stem", "verb-plain"].contains(&k.as_str())
                && own_heads(matched, e).any(|lead| {
                    let next = matched[lead.len()..].chars().next();
                    Head::Text(lead.clone()).is(k)
                        && e.keys().any(|key| {
                            key.strip_prefix(lead.as_str())
                                .is_some_and(|rest| rest.chars().next() == next)
                        })
                })
        })
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
            // A volitional's ending holds its row kana: 習|おう, so
            // おうと思う starts there. Only お, which the points spell
            // (not こう of 聞こう, which reads as こう "this way").
            let start_from = if w.reasons.contains(Reason::Volitional)
                && chars.get(w.start + w.stem) == Some(&'お')
            {
                w.start + w.stem
            } else {
                ending
            };
            for p in start_from..end {
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

    /// Ends a sentence or a bracketed part, or a clause (、) unless
    /// `commas`: a slot doesn't reach past it (は実行され、テストが,
    /// は(実装対象によりますが).
    fn stop(&self, p: usize, commas: bool) -> bool {
        match self.chars.get(p) {
            Some('、' | '，') => !commas,
            Some(c) => "。！？!?\n()（）「」『』".contains(*c),
            None => false,
        }
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
    fn slots_at(
        &self,
        start: usize,
        pieces: &[String],
        commas: bool,
    ) -> Option<Vec<(usize, usize)>> {
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
                if self.stop(p - 1, commas) || self.words_in(at, p) > MAX_SLOT_WORDS {
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
