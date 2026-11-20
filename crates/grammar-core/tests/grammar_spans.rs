//! How well a scan of a sentence finds its grammar, measured against the
//! `{…}` mark of every example in `data/grammar.toml`: the mark says which
//! point the sentence shows and exactly where. A report, not a pass/fail
//! test, to compare a change to the matching with the last run:
//!
//! ```sh
//! cargo test --release -p grammar-core --test grammar_spans -- --ignored --nocapture
//! ```
//!
//! The scan is what the matching gives today: at each char of the
//! sentence, the longest pattern starting there (`lookup_longest_match`;
//! not `hover_match`, whose leaving out one-char and `hover = false` points
//! is the popup's choice, not a miss). Only the marked point is known
//! per sentence; other matches may be right too, so "other matches" counts
//! noise without calling each one wrong.

#![allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::indexing_slicing,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use std::collections::{BTreeMap, HashMap};

use grammar_core::{AnswerCheck, GrammarDict, GrammarEntry, Span, compile};

const MAX_CHARS: usize = 24;

/// What became of one marked example.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Outcome {
    /// The point, on exactly the marked text.
    Exact,
    /// The point, on a span that overlaps the mark but isn't it.
    Shifted,
    /// Something else is the longest match where the mark starts; the
    /// marked text alone does match the point, so a longer or overlapping
    /// pattern hid it.
    Hidden,
    /// The marked text doesn't match the point at all, even alone: its
    /// keys don't reach this spelling (or the point isn't a string, like a
    /// conjugation).
    Unreachable,
}

/// `(start, len)` in chars of the mark, and the plain sentence.
fn marked(ja: &[Span]) -> Option<(String, usize, usize)> {
    let mut start = 0;
    let mut found = None;
    for s in ja {
        let n = s.text().chars().count();
        if let Span::Highlight { .. } = s {
            found = Some((start, n));
        }
        start += n;
    }
    let (s, n) = found?;
    Some((grammar_core::types::plain(ja), s, n))
}

/// Every `(start, len, ids)` the scan reports in `text`.
fn scan(
    text: &str,
    matcher: &impl Fn(&str) -> Option<(Vec<GrammarEntry>, usize)>,
) -> Vec<(usize, usize, Vec<String>)> {
    text.char_indices()
        .enumerate()
        .filter_map(|(c, (b, _))| {
            let (es, n) = matcher(&text[b..])?;
            Some((c, n, es.into_iter().map(|e| e.id).collect()))
        })
        .collect()
}

fn outcome(
    dict: &GrammarDict,
    entry: &GrammarEntry,
    text: &str,
    start: usize,
    len: usize,
    found: &[(usize, usize, Vec<String>)],
) -> Outcome {
    let has = |ids: &[String]| ids.contains(&entry.id);
    if found
        .iter()
        .any(|(s, n, ids)| (*s, *n) == (start, len) && has(ids))
    {
        return Outcome::Exact;
    }
    if found
        .iter()
        .any(|(s, n, ids)| *s < start + len && s + n > start && has(ids))
    {
        return Outcome::Shifted;
    }
    let mark: String = text.chars().skip(start).take(len).collect();
    match dict.lookup_longest_match(&mark, MAX_CHARS) {
        Some((es, n)) if n == len && es.iter().any(|e| e.id == entry.id) => Outcome::Hidden,
        _ => Outcome::Unreachable,
    }
}

fn kind(e: &GrammarEntry) -> &'static str {
    match e.check {
        AnswerCheck::Forms => "forms",
        AnswerCheck::Inflection { .. } => "inflection",
        AnswerCheck::SelfGraded => "self-graded",
    }
}

#[derive(Default)]
struct Tally {
    by: BTreeMap<Outcome, usize>,
}

impl Tally {
    fn add(&mut self, o: Outcome) {
        *self.by.entry(o).or_default() += 1;
    }
    fn total(&self) -> usize {
        self.by.values().sum()
    }
    fn line(&self, name: &str) -> String {
        let t = self.total().max(1) as f64;
        let pct = |o| *self.by.get(&o).unwrap_or(&0) as f64 * 100.0 / t;
        format!(
            "{name:<14} {:5} examples  exact {:5.1}%  shifted {:4.1}%  hidden {:5.1}%  unreachable {:5.1}%",
            self.total(),
            pct(Outcome::Exact),
            pct(Outcome::Shifted),
            pct(Outcome::Hidden),
            pct(Outcome::Unreachable),
        )
    }
}

#[test]
#[ignore = "a report: run with --ignored --nocapture"]
fn grammar_found_where_the_examples_mark_it() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/grammar.toml");
    let compiled = compile(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dict = GrammarDict::from_bytes(&compiled.bytes).unwrap();
    // Seqs are never reused, so they have gaps; 100k is far past the last.
    let entries: Vec<GrammarEntry> = (1..100_000).filter_map(|seq| dict.by_seq(seq)).collect();
    assert_eq!(entries.len(), dict.len());

    let matcher = |t: &str| dict.lookup_longest_match(t, MAX_CHARS);
    {
        let mut all = Tally::default();
        let mut by_kind: BTreeMap<&str, Tally> = BTreeMap::new();
        let mut by_level: BTreeMap<String, Tally> = BTreeMap::new();
        // id → (outcomes, one sentence it missed).
        let mut per_entry: HashMap<String, (Tally, Option<String>)> = HashMap::new();
        let (mut sentences, mut other_matches) = (0, 0);

        for e in &entries {
            for ex in &e.examples {
                let Some((text, start, len)) = marked(&ex.ja) else {
                    continue;
                };
                let found = scan(&text, &matcher);
                let o = outcome(&dict, e, &text, start, len, &found);
                sentences += 1;
                other_matches += found
                    .iter()
                    .filter(|(s, n, ids)| {
                        !(*s < start + len && s + n > start && ids.contains(&e.id))
                    })
                    .count();
                all.add(o);
                by_kind.entry(kind(e)).or_default().add(o);
                by_level.entry(e.jlpt.clone()).or_default().add(o);
                let slot = per_entry.entry(e.id.clone()).or_default();
                slot.0.add(o);
                if o != Outcome::Exact && slot.1.is_none() {
                    let shown: Vec<String> = found
                        .iter()
                        .filter(|(s, n, _)| *s < start + len && s + n > start)
                        .map(|(s, n, ids)| {
                            let t: String = text.chars().skip(*s).take(*n).collect();
                            format!("{t}={}", ids.join("/"))
                        })
                        .collect();
                    let mark: String = text.chars().skip(start).take(len).collect();
                    slot.1 = Some(format!(
                        "{o:?}: {{{mark}}} in {text}  — found: {}",
                        if shown.is_empty() {
                            "nothing".to_owned()
                        } else {
                            shown.join(", ")
                        }
                    ));
                }
            }
        }

        eprintln!("{}", all.line("all"));
        for (k, t) in &by_kind {
            eprintln!("{}", t.line(k));
        }
        for (l, t) in by_level.iter().rev() {
            eprintln!("{}", t.line(l));
        }
        eprintln!(
            "other matches per sentence: {:.2}",
            other_matches as f64 / sentences.max(1) as f64
        );

        let mut worst: Vec<_> = per_entry
            .iter()
            .filter(|(_, (t, _))| t.total() >= 3)
            .map(|(id, (t, miss))| {
                let exact = *t.by.get(&Outcome::Exact).unwrap_or(&0);
                (exact as f64 / t.total() as f64, id, miss)
            })
            .filter(|(r, _, _)| *r < 1.0)
            .collect();
        worst.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        eprintln!(
            "{} points miss at least one example; the worst:",
            worst.len()
        );
        for (r, id, miss) in worst.iter().take(40) {
            eprintln!(
                "  {:3.0}%  {id:<28} {}",
                r * 100.0,
                miss.as_deref().unwrap_or("")
            );
        }
    }
}
