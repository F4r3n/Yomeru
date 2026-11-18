mod reason;
mod rules;
mod suffix_trie;

pub use reason::{Reason, Reasons};
use rules::RULES;

/// Result of deinflecting a word.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Deinflected {
    /// Candidate dictionary form (e.g. "食べる").
    pub text: String,
}

/// A [`Deinflected`] form with the rules that reach it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeinflectedWithReasons {
    /// Candidate dictionary form (e.g. "食べる").
    pub text: String,
    /// Every step of every path from the input to `text`: 食べさせられた →
    /// 食べる is causative, passive and past. Empty for the input itself.
    pub reasons: Reasons,
}

/// Return every plausible dictionary form of `text` by repeatedly applying
/// deinflection rules. The original form is always included first.
/// Results are deduplicated and ordered from least to most transformed.
///
/// Allocates its result; a caller deinflecting many words in a row should
/// keep a [`Deinflector`] instead.
pub fn deinflect(text: &str) -> Vec<Deinflected> {
    Deinflector::default()
        .run(text)
        .map(|t| Deinflected { text: t.to_owned() })
        .collect()
}

/// [`deinflect`], with the reasons of each form. Most callers only need the
/// forms: this one is for asking which form a word is in (is 食べさせた a
/// causative?).
pub fn deinflect_with_reasons(text: &str) -> Vec<DeinflectedWithReasons> {
    Deinflector::default()
        .run_with_reasons(text)
        .map(|(t, reasons)| DeinflectedWithReasons {
            text: t.to_owned(),
            reasons,
        })
        .collect()
}

/// Whether some rule applies to `text`. When none does, [`deinflect`] gives
/// back only `text` itself: a caller that already knows `text` isn't a word
/// can skip deinflecting it.
pub fn can_deinflect(text: &str) -> bool {
    suffix_trie::rules_trie().any_match(text)
}

/// Reusable deinflection buffers: [`Deinflector::run`] gives the same forms
/// as [`deinflect`], in the same order, but keeps its memory between calls.
///
/// Every candidate is written into one shared string, and the list of their
/// ranges is also the breadth-first queue: a candidate's own rules are tried
/// when the walk through the list reaches it. A word has a few dozen
/// candidates at most, so a duplicate is found by comparing against them
/// rather than hashing.
#[derive(Debug, Default)]
pub struct Deinflector {
    /// Every candidate, end to end.
    buf: String,
    /// (start, end, depth) of each candidate in `buf`, in the order found.
    forms: Vec<(u32, u32, u8)>,
    /// The reasons of each of `forms`, kept by [`Deinflector::run_with_reasons`]
    /// only.
    reasons: Vec<Reasons>,
    /// The rules matching the candidate being expanded.
    matched: Vec<u16>,
}

/// Rules are applied at most this many times in a row.
const MAX_DEPTH: u8 = 3;

impl Deinflector {
    /// The forms of `text`, itself first, then each new form in the order
    /// found (least transformed first).
    pub fn run(&mut self, text: &str) -> impl Iterator<Item = &str> + '_ {
        self.walk::<false>(text);
        self.forms.iter().map(|&(s, e, _)| slice(&self.buf, s, e))
    }

    /// The forms of [`Deinflector::run`], in the same order, each with its
    /// reasons.
    pub fn run_with_reasons(&mut self, text: &str) -> impl Iterator<Item = (&str, Reasons)> + '_ {
        self.walk::<true>(text);
        self.merge_paths();
        self.forms
            .iter()
            .zip(&self.reasons)
            .map(|(&(s, e, _), &r)| (slice(&self.buf, s, e), r))
    }

    /// The breadth-first search. Without `REASONS`, a form found again is
    /// dropped. With it, a form found again by a path that adds reasons is
    /// kept and expanded as well (its own forms carry those reasons too);
    /// [`Deinflector::merge_paths`] then folds the copies into the first.
    fn walk<const REASONS: bool>(&mut self, text: &str) {
        self.buf.clear();
        self.forms.clear();
        self.reasons.clear();
        // Don't produce single-kana candidates from multi-char input (avoids
        // garbage like "い" matching half a word).
        let min_chars = if text.chars().count() > 2 { 2 } else { 1 };
        self.buf.push_str(text);
        self.forms.push((0, text.len() as u32, 0));
        if REASONS {
            self.reasons.push(Reasons::EMPTY);
        }

        let trie = suffix_trie::rules_trie();
        let mut next = 0;
        while let Some(&(start, end, depth)) = self.forms.get(next) {
            let parent = next;
            next += 1;
            if depth >= MAX_DEPTH {
                continue;
            }
            let candidate = slice(&self.buf, start, end);
            trie.matches(candidate, &mut self.matched);
            for &r in &self.matched {
                let Some(rule) = RULES.get(usize::from(r)) else {
                    continue;
                };
                let Some(stem_len) = slice(&self.buf, start, end)
                    .strip_suffix(rule.suffix_in)
                    .map(str::len)
                else {
                    continue;
                };
                // Write the new form at the end of the buffer, and take it
                // back off if it's too short or already known.
                let new_start = self.buf.len();
                let stem_start = start as usize;
                self.buf
                    .extend_from_within(stem_start..stem_start + stem_len);
                self.buf.push_str(rule.suffix_out);
                let new_end = self.buf.len() as u32;
                let new = slice(&self.buf, new_start as u32, new_end);
                let reasons = if REASONS {
                    self.reasons
                        .get(parent)
                        .copied()
                        .unwrap_or_default()
                        .union(rule.reasons)
                } else {
                    Reasons::EMPTY
                };
                let known = self.forms.iter().enumerate().any(|(i, &(s, e, _))| {
                    slice(&self.buf, s, e) == new
                        && (!REASONS
                            || self.reasons.get(i).is_some_and(|r| r.contains_all(reasons)))
                });
                if known || new.chars().count() < min_chars {
                    self.buf.truncate(new_start);
                    continue;
                }
                self.forms.push((new_start as u32, new_end, depth + 1));
                if REASONS {
                    self.reasons.push(reasons);
                }
            }
        }
    }

    /// Folds each form found again into its first copy, joining their
    /// reasons, so every text is listed once and in [`Deinflector::run`]'s
    /// order: a copy is always found after the first, and its own forms
    /// after the first's.
    fn merge_paths(&mut self) {
        let mut kept = 0;
        for i in 0..self.forms.len() {
            let (Some(&form), Some(&reasons)) = (self.forms.get(i), self.reasons.get(i)) else {
                continue;
            };
            let text = slice(&self.buf, form.0, form.1);
            let first = self
                .forms
                .get(..kept)
                .unwrap_or_default()
                .iter()
                .position(|&(s, e, _)| slice(&self.buf, s, e) == text);
            match first {
                Some(j) => {
                    if let Some(r) = self.reasons.get_mut(j) {
                        *r = r.union(reasons);
                    }
                }
                None => {
                    if let Some(f) = self.forms.get_mut(kept) {
                        *f = form;
                    }
                    if let Some(r) = self.reasons.get_mut(kept) {
                        *r = reasons;
                    }
                    kept += 1;
                }
            }
        }
        self.forms.truncate(kept);
        self.reasons.truncate(kept);
    }
}

/// `buf[start..end]`, or "" for a range that isn't one (never, by
/// construction).
fn slice(buf: &str, start: u32, end: u32) -> &str {
    buf.get(start as usize..end as usize).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(text: &str) -> Vec<String> {
        deinflect(text).into_iter().map(|d| d.text).collect()
    }

    #[test]
    fn ichidan_negative() {
        assert!(forms("食べない").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_past() {
        assert!(forms("食べた").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_te_form() {
        assert!(forms("食べて").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_passive() {
        assert!(forms("食べられる").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_negative_past() {
        assert!(forms("食べなかった").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_chained_passive_negative() {
        // 食べられなかった → 食べられる → 食べる  (two steps)
        assert!(forms("食べられなかった").contains(&"食べる".to_owned()));
    }

    #[test]
    fn godan_ku_negative() {
        assert!(forms("書かない").contains(&"書く".to_owned()));
    }

    #[test]
    fn godan_ku_past() {
        assert!(forms("書いた").contains(&"書く".to_owned()));
    }

    #[test]
    fn godan_mu_progressive() {
        assert!(forms("飲んでいる").contains(&"飲む".to_owned()));
    }

    #[test]
    fn godan_ru_polite() {
        assert!(forms("帰ります").contains(&"帰る".to_owned()));
    }

    #[test]
    fn adjective_negative() {
        assert!(forms("高くない").contains(&"高い".to_owned()));
    }

    #[test]
    fn adjective_past() {
        assert!(forms("高かった").contains(&"高い".to_owned()));
    }

    #[test]
    fn suru_verb() {
        assert!(forms("勉強した").contains(&"勉強する".to_owned()));
    }

    #[test]
    fn suru_noun_past_chains_to_stem() {
        // 流出した → 流出する (した→する) → 流出 (する→"")
        let f = forms("流出した");
        assert!(f.contains(&"流出する".to_owned()));
        assert!(f.contains(&"流出".to_owned()));
    }

    #[test]
    fn suru_noun_present_strips_to_stem() {
        assert!(forms("流出する").contains(&"流出".to_owned()));
    }

    #[test]
    fn iku_irregular_te_ta_forms_come_first() {
        // The caller takes the first candidate that hits the dictionary, and
        // 行る (via った→る) is a real headword, so 行く must be tried before it.
        for surface in ["行った", "行って", "行っている", "行っていた"] {
            assert_eq!(
                deinflect(surface)[1].text,
                "行く",
                "{surface} should deinflect to 行く before anything else"
            );
        }
    }

    #[test]
    fn literary_negative_zu() {
        for (surface, base) in [
            ("足りず", "足りる"),
            ("食べず", "食べる"),
            ("行かず", "行く"),
            ("分からず", "分かる"),
            ("言わず", "言う"),
            ("飲まず", "飲む"),
            ("こず", "くる"),
            ("来ず", "来る"),
            ("勉強せず", "勉強する"),
        ] {
            assert!(
                forms(surface).contains(&base.to_owned()),
                "{surface} should deinflect to {base}"
            );
        }
    }

    #[test]
    fn without_doing_zuni_chains_through_zu() {
        assert!(forms("食べずに").contains(&"食べる".to_owned()));
        assert!(forms("言わずに").contains(&"言う".to_owned()));
    }

    /// The reasons `text` reaches `base` with.
    fn reasons(text: &str, base: &str) -> Reasons {
        deinflect_with_reasons(text)
            .into_iter()
            .find(|d| d.text == base)
            .unwrap_or_else(|| panic!("{text} should deinflect to {base}"))
            .reasons
    }

    #[test]
    fn volitional() {
        for (surface, base) in [
            ("食べよう", "食べる"),
            ("行こう", "行く"),
            ("泳ごう", "泳ぐ"),
            ("話そう", "話す"),
            ("待とう", "待つ"),
            ("死のう", "死ぬ"),
            ("遊ぼう", "遊ぶ"),
            ("飲もう", "飲む"),
            ("帰ろう", "帰る"),
            ("買おう", "買う"),
            ("勉強しよう", "勉強する"),
            ("こよう", "くる"),
            ("来よう", "来る"),
            ("食べましょう", "食べる"),
            ("行きましょう", "行く"),
            ("しましょう", "する"),
        ] {
            assert!(
                reasons(surface, base).contains(Reason::Volitional),
                "{surface} → {base} should be volitional"
            );
        }
        assert!(reasons("行きましょう", "行く").contains(Reason::Polite));
    }

    #[test]
    fn short_causative_passive_imperatives_and_nasai() {
        for (surface, base, want) in [
            (
                "待たされた",
                "待つ",
                Reasons::of(&[Reason::Causative, Reason::Passive, Reason::Past]),
            ),
            (
                "歌わされる",
                "歌う",
                Reasons::of(&[Reason::Causative, Reason::Passive]),
            ),
            ("来い", "来る", Reasons::of(&[Reason::Imperative])),
            ("こい", "くる", Reasons::of(&[Reason::Imperative])),
            ("勉強しろ", "勉強する", Reasons::of(&[Reason::Imperative])),
            ("せよ", "する", Reasons::of(&[Reason::Imperative])),
            ("食べなさい", "食べる", Reasons::of(&[Reason::Nasai])),
            ("読み返しなさい", "読み返す", Reasons::of(&[Reason::Nasai])),
            ("勉強しなさい", "勉強する", Reasons::of(&[Reason::Nasai])),
            ("忘れていった", "忘れていく", Reasons::of(&[Reason::Past])),
            ("飛んでいって", "飛んでいく", Reasons::of(&[Reason::TeForm])),
        ] {
            assert!(
                reasons(surface, base).contains_all(want),
                "{surface} → {base}"
            );
        }
    }

    #[test]
    fn reasons_join_along_the_chain() {
        let r = reasons("食べさせられなかった", "食べる");
        for want in [
            Reason::Causative,
            Reason::Passive,
            Reason::Negative,
            Reason::Past,
        ] {
            assert!(r.contains(want), "{want:?} missing from {r:?}");
        }
        assert!(!r.contains(Reason::Polite));
        assert_eq!(
            reasons("書かせた", "書く"),
            Reasons::of(&[Reason::Causative, Reason::Past])
        );
        assert_eq!(reasons("書ける", "書く"), Reasons::of(&[Reason::Potential]));
        assert_eq!(
            reasons("勉強できる", "勉強する"),
            Reasons::of(&[Reason::Potential])
        );
        assert_eq!(reasons("書け", "書く"), Reasons::of(&[Reason::Imperative]));
        assert!(reasons("取られる", "取る").contains(Reason::Passive));
        assert_eq!(reasons("食べる", "食べる"), Reasons::EMPTY);
    }

    #[test]
    fn a_form_reached_twice_keeps_the_reasons_of_both_paths() {
        // させられる → る is causative-passive; られる → る then させる → る
        // also reaches 食べる, through the passive/potential られる.
        let r = reasons("食べさせられる", "食べる");
        assert!(r.contains_all(Reasons::of(&[
            Reason::Causative,
            Reason::Passive,
            Reason::Potential
        ])));
    }

    #[test]
    fn with_reasons_gives_the_forms_of_run_in_the_same_order() {
        let mut words: Vec<String> = [
            "食べさせられなかった",
            "食べさせられる",
            "行きましょう",
            "hello",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for r in RULES {
            words.push(r.suffix_in.to_string());
            words.push(format!("食べ{}", r.suffix_in));
            words.push(format!("書{}", r.suffix_in));
        }
        let mut d = Deinflector::default();
        for w in &words {
            let plain: Vec<String> = d.run(w).map(str::to_owned).collect();
            let with: Vec<String> = d.run_with_reasons(w).map(|(t, _)| t.to_owned()).collect();
            assert_eq!(with, plain, "forms of {w:?}");
        }
    }

    #[test]
    fn kuru_verb() {
        assert!(forms("来なかった").contains(&"来る".to_owned()));
    }

    /// The breadth-first search as it was before [`Deinflector`]: a queue of
    /// owned strings, a hash set, and every rule tested. Kept to check the
    /// faster one against.
    fn reference(text: &str) -> Vec<String> {
        use std::collections::{HashSet, VecDeque};
        let mut queue: VecDeque<(String, u8)> = VecDeque::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut results = Vec::new();
        let input_char_count = text.chars().count();
        queue.push_back((text.to_owned(), 0));
        while let Some((candidate, depth)) = queue.pop_front() {
            if !seen.insert(candidate.clone()) {
                continue;
            }
            results.push(candidate.clone());
            if depth >= 3 {
                continue;
            }
            for rule in RULES {
                let Some(stem) = candidate.strip_suffix(rule.suffix_in) else {
                    continue;
                };
                let new_text = format!("{}{}", stem, rule.suffix_out);
                let n = new_text.chars().count();
                if n < 1 || (input_char_count > 2 && n < 2) || seen.contains(&new_text) {
                    continue;
                }
                queue.push_back((new_text, depth + 1));
            }
        }
        results
    }

    #[test]
    fn same_forms_in_the_same_order_as_the_reference() {
        let mut words: Vec<String> = [
            "食べさせられなかった",
            "書いていなかった",
            "飲んでいなかった",
            "勉強しました",
            "来なかった",
            "行かなければならない",
            "美しくなかった",
            "静かでした",
            "テスト",
            "hello",
            "",
            "た",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // Every rule's own suffix, alone and after a stem, reaches every
        // rule and the chains it starts.
        for r in RULES {
            words.push(r.suffix_in.to_string());
            words.push(format!("食べ{}", r.suffix_in));
            words.push(format!("書{}", r.suffix_in));
        }
        // One deinflector for every word: reuse must not leak between runs.
        let mut d = Deinflector::default();
        for w in &words {
            let fast: Vec<String> = d.run(w).map(str::to_owned).collect();
            assert_eq!(fast, reference(w), "forms of {w:?}");
        }
    }
}
