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
        let text =
            |start: usize, len: usize| -> String { chars.iter().skip(start).take(len).collect() };
        // Where the plain search is trusted for any point: a phrase kept
        // as one word, a longer point's text.
        let holders: Vec<String> = breakdown
            .words
            .iter()
            .filter(|w| {
                w.len >= 2
                    && w.entry
                        .is_some_and(|e| segment::classes(e).contains(&Class::Expression))
            })
            .map(|w| text(w.start, w.len))
            .chain(
                found
                    .iter()
                    .filter(|m| m.kind != MatchKind::Slots)
                    .map(|m| text(m.start, m.len)),
            )
            .collect();
        // A suffix or prefix point (型 of 大型車) in any word holding it.
        let words: Vec<String> = breakdown
            .words
            .iter()
            .filter(|w| w.len >= 2)
            .map(|w| text(w.start, w.len))
            .collect();
        let held = |e: &GrammarEntry| {
            let affix = e.suffix || e.prefix;
            e.keys().filter(|k| !k.contains('〜')).any(|k| {
                holders
                    .iter()
                    .chain(words.iter().filter(|_| affix))
                    .any(|h| h.len() > k.len() && h.contains(k))
            })
        };
        let uses = |e: &GrammarEntry| {
            found_ids().any(|f| f == e.id)
                || ((plain_trusted(e) || held(e))
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
/// for `e`: a pattern of [`PLAIN_MIN_CHARS`] or more, not a verb form.
fn plain_trusted(e: &GrammarEntry) -> bool {
    e.check == AnswerCheck::Forms && e.pattern.replace('〜', "").chars().count() >= PLAIN_MIN_CHARS
}
