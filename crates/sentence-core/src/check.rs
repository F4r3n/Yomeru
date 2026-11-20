//! The production review's answer check, on the breakdown: does a typed
//! sentence use the card's point?
//!
//! The breakdown finds a point where it stands: a particle as its own
//! word (the は of 彼はもう…, which [`grammar_core::GrammarDict::check_answer`]'s plain
//! search reads as part of はも), not inside another (方 of 方法 isn't
//! 方 "way of doing"), a verb form on a verb (not a command form in
//! 話しかける). It misses a few points the plain search finds, so that
//! search still accepts an answer for a point of three chars or more
//! that is a pattern rather than a verb form, where it rarely errs; and
//! for any point it finds within a phrase JMdict lists whole (暇なとき,
//! 楽しみに待つ), which the breakdown keeps as one word, or within a
//! longer point the breakdown shows instead (こと of ことです).

use grammar_core::{AnswerCheck, GrammarEntry, Related, Verdict, Vocabulary};

use jmdict_core::segment::{self, Class};

use crate::{Analyzer, MatchKind};

/// The shortest pattern, in chars (less its `〜`), the plain search alone
/// accepts.
const PLAIN_MIN_CHARS: usize = 3;

/// The shortest match named as another point the answer uses (the
/// popup's rule: は, に, を aren't named).
const OTHER_MIN_CHARS: usize = 2;

impl Analyzer<'_> {
    /// Checks a typed production answer for the point `id`: accepted when
    /// it uses the point, close when it uses one of the point's `related`
    /// instead, other when it uses other points.
    pub fn check_answer(&self, id: &str, answer: &str, vocab: &impl Vocabulary) -> Verdict {
        let dict = self.dict;
        let Some(entry) = dict.by_id(id) else {
            return Verdict::SelfGraded;
        };
        if entry.check == AnswerCheck::SelfGraded {
            return Verdict::SelfGraded;
        }
        let answer: String = answer.chars().filter(|c| !c.is_whitespace()).collect();
        let breakdown = self.analyze(&answer);
        let found = &breakdown.grammar;
        let found_ids = || {
            found
                .iter()
                .flat_map(|m| m.entries.iter().map(|e| e.id.as_str()))
        };
        let chars: Vec<char> = answer.chars().collect();
        // Where the plain search is trusted for any point:
        // - a phrase kept as one word (暇なとき, 楽しみに待つ), but not
        //   at its start unless the key ends it or is its dictionary form
        //   (では of ではない, どう of どうしても, それ of それに; 行きつ戻りつ
        //   of 行きつ戻りつして),
        //   and a one-char key only at its end (と of になると, not か of
        //   かもしれない), for a point not after an amount (number-wa's
        //   は of には);
        // - a longer point's text (こと of ことです), on its words' edges,
        //   or a conjugation's (ずに of れずに), for a key of two chars or
        //   more; one char only as a word of its own (を of ことを, に of
        //   上に) and for a point with no `after` (number-wa's は of
        //   ことは, a piece);
        // - for a suffix or prefix point, any word (型 of 大型車).
        #[derive(Clone, Copy, PartialEq)]
        enum Holder<'w> {
            /// With the phrase's dictionary form (行きつ戻りつ of
            /// 行きつ戻りつして).
            Phrase(&'w str),
            Point {
                on_edges: bool,
            },
            Word,
        }
        let holders: Vec<(usize, usize, Holder<'_>)> = breakdown
            .words
            .iter()
            .filter(|w| {
                w.len >= 2
                    && w.entry
                        .is_some_and(|e| segment::classes(e).contains(&Class::Expression))
            })
            .map(|w| (w.start, w.len, Holder::Phrase(w.base.as_str())))
            .chain(
                found
                    .iter()
                    .filter(|m| m.kind != MatchKind::Slots)
                    .map(|m| {
                        let on_edges = m.kind == MatchKind::Text;
                        (m.start, m.len, Holder::Point { on_edges })
                    }),
            )
            .collect();
        let words: Vec<(usize, usize, Holder<'_>)> = breakdown
            .words
            .iter()
            .filter(|w| w.len >= 2)
            .map(|w| (w.start, w.len, Holder::Word))
            .collect();
        let edge = |p: usize| {
            breakdown
                .words
                .iter()
                .any(|w| w.start == p || w.start + w.len == p)
        };
        // A one-char word of its own (を of ことを, に of 上に); kana only:
        // a kanji there is a noun (中 of 中を isn't 中 "throughout").
        let lone = |p: usize| breakdown.words.iter().any(|w| w.start == p && w.len == 1);
        let held = |e: &GrammarEntry| {
            let affix = e.suffix || e.prefix;
            e.keys().filter(|k| !k.contains('〜')).any(|k| {
                let key: Vec<char> = k.chars().collect();
                let n = key.len();
                holders
                    .iter()
                    .chain(words.iter().filter(|_| affix))
                    .any(|&(start, len, holder)| {
                        len > n
                            && (start..=start + len - n).any(|p| {
                                chars.get(p..p + n) == Some(&key[..])
                                    && match holder {
                                        Holder::Phrase(base) => {
                                            if n >= 2 {
                                                p > start || p + n == start + len || k == base
                                            } else {
                                                p + n == start + len
                                                    && !e.after.iter().any(|a| a == "amount")
                                            }
                                        }
                                        Holder::Point { on_edges } => {
                                            if n >= 2 {
                                                !on_edges || (edge(p) && edge(p + n))
                                            } else {
                                                lone(p)
                                                    && e.after.is_empty()
                                                    && key.iter().all(|c| ('ぁ'..='ゖ').contains(c))
                                            }
                                        }
                                        Holder::Word => true,
                                    }
                            })
                    })
            })
        };
        // A slot pattern (に〜ない) is trusted only when its first piece
        // ends a point the breakdown found (のは of のは〜だ, も of も〜ば〜も)
        // or lies in a phrase: alone the plain search spans a clause
        // (に of 子供たちに … ない of 当てはまらない).
        let starts = |e: &GrammarEntry| {
            e.keys().filter(|k| k.contains('〜')).any(|k| {
                let pieces: Vec<Vec<char>> = k
                    .split('〜')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.chars().collect())
                    .collect();
                let Some(first) = pieces.first() else {
                    return false;
                };
                let n = first.len();
                let at = |p: usize, piece: &[char]| chars.get(p..p + piece.len()) == Some(piece);
                found
                    .iter()
                    .any(|m| m.len >= n && at(m.start + m.len - n, first))
                    || holders.iter().any(|&(start, len, holder)| {
                        let end = start + len;
                        matches!(holder, Holder::Phrase(_))
                            && ((start..=end.saturating_sub(n))
                                .any(|p| at(p, first) && (n >= 2 || p + n == end))
                                || chars.get(start..end).is_some_and(|t| in_order(t, &pieces)))
                    })
            })
        };
        // Where the plain search alone finds a pattern, the breakdown
        // must agree on its edges:
        // - not cut short by a longer point it found from the same char
        //   (もので of ものではない under もの, として of としている under
        //   とする, つもりで of つもりです);
        // - ending on a word edge, a char into the next word (だ of
        //   おかげだった, に of になった), two into a function word (です
        //   of ですが), or inside the word it starts (そうです of
        //   そうですね), not inside some other word (どう of どうしても,
        //   をもって of をもて遊ぶ);
        // - starting on a word edge or after a stem (ていった of
        //   忘れていった, そうだ of うれしそうだ), but not on the ん of a
        //   verb or adjective (んだ of 読んだ: the explanatory ん is a word
        //   of its own; なんだって, 狂わんばかり are not).
        let word_at = |p: usize| {
            breakdown
                .words
                .iter()
                .find(|w| w.start <= p && p < w.start + w.len)
        };
        let on_edges = |e: &GrammarEntry| {
            dict.key_spans(e, &answer).iter().any(|&(s, end)| {
                let cut_short = found
                    .iter()
                    .any(|m| m.kind == MatchKind::Text && m.start == s && end < m.start + m.len);
                let ends = edge(end)
                    || word_at(end - 1).is_some_and(|w| {
                        w.start == s
                            || end == w.start + 1
                            || (end == w.start + 2
                                && !matches!(w.class, Class::Verb | Class::Adjective | Class::Noun))
                    });
                let begins = edge(s)
                    || chars.get(s) != Some(&'ん')
                    || word_at(s)
                        .is_some_and(|w| !matches!(w.class, Class::Verb | Class::Adjective));
                !cut_short && ends && begins
            })
        };
        let uses = |e: &GrammarEntry| {
            found_ids().any(|f| f == e.id)
                || ((plain_trusted(e) && (if has_slot(e) { starts(e) } else { on_edges(e) })
                    || held(e))
                    && dict.check_answer(&e.id, &answer, vocab) == Verdict::Accepted)
        };
        if uses(&entry) {
            return Verdict::Accepted;
        }
        for r in &entry.related {
            if dict.by_id(&r.id).is_some_and(|e| uses(&e)) {
                return Verdict::Close { related: r.clone() };
            }
        }
        let mut others: Vec<Related> = Vec::new();
        for m in found.iter().filter(|m| m.len >= OTHER_MIN_CHARS) {
            for e in &m.entries {
                if e.id != entry.id && !others.iter().any(|o| o.id == e.id) {
                    others.push(Related {
                        id: e.id.clone(),
                        pattern: e.pattern.clone(),
                    });
                }
            }
        }
        if others.is_empty() {
            Verdict::NotFound
        } else {
            Verdict::Other { found: others }
        }
    }
}

/// Whether [`grammar_core::GrammarDict::check_answer`]'s plain search is trusted alone
/// for `e`: a pattern of [`PLAIN_MIN_CHARS`] or more, not a verb form,
/// and for a slot pattern not a template its forms spell out whole
/// (に〜ない: 泣くに泣けない, 笑うに笑えない).
fn plain_trusted(e: &GrammarEntry) -> bool {
    e.check == AnswerCheck::Forms
        && e.pattern.replace('〜', "").chars().count() >= PLAIN_MIN_CHARS
        && !(has_slot(e) && !e.forms.is_empty() && e.forms.iter().all(|f| !f.contains('〜')))
}

/// Whether `e`'s pattern has a slot inside (に〜ない, たり〜たり), not
/// only at its ends.
fn has_slot(e: &GrammarEntry) -> bool {
    e.pattern.trim_matches('〜').contains('〜')
}

/// Whether `pieces` appear in `text` in order, as written.
fn in_order(text: &[char], pieces: &[Vec<char>]) -> bool {
    let mut from = 0;
    pieces.iter().all(|piece| {
        let n = piece.len();
        match (from..=text.len().saturating_sub(n))
            .find(|&p| text.get(p..p + n) == Some(&piece[..]))
        {
            Some(p) => {
                from = p + n;
                true
            }
            None => false,
        }
    })
}
