//! How well the breakdown finds a sentence's grammar, measured against the
//! `{…}` mark of every example in `data/grammar.toml`, as
//! `grammar-core/tests/grammar_spans.rs` measures the plain scan. A report:
//!
//! ```sh
//! JMDICT_BIN=path/to/jmdict.bin cargo test --release -p sentence-core \
//!     --test grammar_breakdown -- --ignored --nocapture
//! ```
//!
//! `JMDICT_BIN` must be a `jmdict-types/full` build. `GRAMMAR_REPORT=path`
//! also writes every example the breakdown misses or finds elsewhere, and
//! `GRAMMAR_NOISE=path` the commonest other matches. `GRAMMAR_BEFORE=id,id`
//! prints the word before each match of those points (for `after`).
//!
//! Only the marked point is known per sentence; other matches may be right
//! too, so "other matches" counts what a reader sees besides it.

#![allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use grammar_core::{AnswerCheck, GrammarDict, GrammarEntry, Span, compile};
use sentence_core::{Analyzer, GrammarMatch, MatchKind};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Outcome {
    /// The point, on exactly the marked text.
    Exact,
    /// The point, on a span overlapping the mark.
    Overlap,
    Missed,
}

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

fn overlaps(m: &GrammarMatch, start: usize, len: usize) -> bool {
    m.start < start + len && m.start + m.len > start
}

/// Whether an overlap is only the example's mark and the pattern covering
/// different stretches of the same point: one span holds the other, and
/// ours is the entry's pattern as written or conjugated (の間に for a mark
/// 間に, ぶっている for ぶって) or its slots (だろうが社長だろうが for
/// 学生だろうが社長だろうが). The reader sees the right point there.
fn convention(
    dict: &GrammarDict,
    text: &str,
    m: &GrammarMatch,
    id: &str,
    start: usize,
    len: usize,
) -> bool {
    let holds = |a: usize, n: usize, b: usize, k: usize| a <= b && b + k <= a + n;
    if !has(m, id) || !(holds(m.start, m.len, start, len) || holds(start, len, m.start, m.len)) {
        return false;
    }
    if m.kind == MatchKind::Slots {
        return true;
    }
    let t: String = text.chars().skip(m.start).take(m.len).collect();
    dict.lookup_longest_match(&t, m.len)
        .is_some_and(|(es, n)| n == m.len && es.iter().any(|e| e.id == id))
}

fn has(m: &GrammarMatch, id: &str) -> bool {
    m.entries.iter().any(|e| e.id == id)
}

fn kind(e: &GrammarEntry) -> &'static str {
    match e.check {
        AnswerCheck::Forms if e.pattern.contains('〜') => "forms, slots",
        AnswerCheck::Forms => "forms",
        AnswerCheck::Inflection { .. } => "inflection",
        AnswerCheck::SelfGraded => "self-graded",
    }
}

#[derive(Default)]
struct Tally(BTreeMap<Outcome, usize>);

impl Tally {
    fn add(&mut self, o: Outcome) {
        *self.0.entry(o).or_default() += 1;
    }
    fn total(&self) -> usize {
        self.0.values().sum()
    }
    fn line(&self, name: &str) -> String {
        let t = self.total().max(1) as f64;
        let pct = |o| *self.0.get(&o).unwrap_or(&0) as f64 * 100.0 / t;
        format!(
            "{name:<14} {:5} examples  exact {:5.1}%  overlap {:4.1}%  missed {:5.1}%",
            self.total(),
            pct(Outcome::Exact),
            pct(Outcome::Overlap),
            pct(Outcome::Missed),
        )
    }
}

#[test]
#[ignore = "needs JMDICT_BIN (a `full` build); a report: run with --ignored --nocapture"]
fn grammar_found_on_the_words() {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN to a full jmdict.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("can't read {path}: {e}"));
    jmdict_core::init(&bytes).expect("init: is JMDICT_BIN a `full` build?");
    let toml = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/grammar.toml");
    let compiled = compile(&std::fs::read_to_string(toml).unwrap()).unwrap();
    let dict = GrammarDict::from_bytes(&compiled.bytes).unwrap();
    let analyzer = Analyzer::new(&dict);

    let mut all = Tally::default();
    // The same, an overlap that is only a convention counted as exact.
    let mut aside = Tally::default();
    let mut by_kind: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut by_level: BTreeMap<String, Tally> = BTreeMap::new();
    let mut per_entry: HashMap<String, (Tally, Option<String>)> = HashMap::new();
    let mut noise: HashMap<String, (usize, String)> = HashMap::new();
    let mut by_match_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut sentences, mut other_matches) = (0, 0);
    let mut report = String::new();
    // GRAMMAR_BEFORE=id,id: what the word before each match of those points
    // is, on the marked match and on the others.
    let before_ids: Vec<String> = std::env::var("GRAMMAR_BEFORE")
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let mut before: BTreeMap<(String, bool, String), (usize, Vec<String>)> = BTreeMap::new();

    for e in dict.entries() {
        for ex in &e.examples {
            let Some((text, start, len)) = marked(&ex.ja) else {
                continue;
            };
            let b = analyzer.analyze(&text);
            let o = if b
                .grammar
                .iter()
                .any(|m| (m.start, m.len) == (start, len) && has(m, &e.id))
            {
                Outcome::Exact
            } else if b
                .grammar
                .iter()
                .any(|m| overlaps(m, start, len) && has(m, &e.id))
            {
                Outcome::Overlap
            } else {
                Outcome::Missed
            };
            sentences += 1;
            for m in &b.grammar {
                for id in &before_ids {
                    if !has(m, id) {
                        continue;
                    }
                    let prev = b.words.iter().rfind(|w| w.start < m.start);
                    let what = prev.map_or("start".to_owned(), |w| {
                        let r: Vec<String> = w.reasons.iter().map(|r| format!("{r:?}")).collect();
                        format!("{:?} {}", w.class, r.join("+"))
                    });
                    let marked = *id == e.id && overlaps(m, start, len);
                    let slot = before.entry((id.clone(), marked, what)).or_default();
                    slot.0 += 1;
                    if slot.1.len() < 30 {
                        slot.1.push(text.clone());
                    }
                }
                if overlaps(m, start, len) && has(m, &e.id) {
                    continue;
                }
                other_matches += 1;
                let t: String = text.chars().skip(m.start).take(m.len).collect();
                let ids: Vec<&str> = m.entries.iter().map(|x| x.id.as_str()).collect();
                let slot = noise
                    .entry(format!("{t}={}", ids.join("/")))
                    .or_insert_with(|| (0, text.clone()));
                slot.0 += 1;
                *by_match_kind
                    .entry(match m.kind {
                        MatchKind::Text => "text",
                        MatchKind::Slots => "slots",
                        MatchKind::Conjugation => "conjugation",
                    })
                    .or_default() += 1;
            }
            all.add(o);
            let forgiven = o == Outcome::Overlap
                && b.grammar
                    .iter()
                    .any(|m| convention(&dict, &text, m, &e.id, start, len));
            aside.add(if forgiven { Outcome::Exact } else { o });
            by_kind.entry(kind(&e)).or_default().add(o);
            by_level.entry(e.jlpt.clone()).or_default().add(o);
            let slot = per_entry.entry(e.id.clone()).or_default();
            slot.0.add(o);
            if o != Outcome::Exact && !forgiven {
                let mark: String = text.chars().skip(start).take(len).collect();
                let shown: Vec<String> = b
                    .grammar
                    .iter()
                    .filter(|m| overlaps(m, start, len))
                    .map(|m| {
                        let t: String = text.chars().skip(m.start).take(m.len).collect();
                        let ids: Vec<&str> = m.entries.iter().map(|x| x.id.as_str()).collect();
                        format!("{t}={}", ids.join("/"))
                    })
                    .collect();
                let words: Vec<String> = b
                    .words
                    .iter()
                    .map(|w| text.chars().skip(w.start).take(w.len).collect())
                    .collect();
                let line = format!(
                    "{o:?}: {{{mark}}} in {text}  — words: {}  — found: {}",
                    words.join("|"),
                    if shown.is_empty() {
                        "nothing".to_owned()
                    } else {
                        shown.join(", ")
                    }
                );
                writeln!(report, "{}\t{line}", e.id).unwrap();
                if slot.1.is_none() {
                    slot.1 = Some(line);
                }
            }
        }
    }

    eprintln!("\n{}", all.line("all"));
    eprintln!("{}", aside.line("conventions"));
    for (k, t) in &by_kind {
        eprintln!("{}", t.line(k));
    }
    for (l, t) in by_level.iter().rev() {
        eprintln!("{}", t.line(l));
    }
    eprintln!(
        "other matches per sentence: {:.2} ({})",
        other_matches as f64 / sentences.max(1) as f64,
        by_match_kind
            .iter()
            .map(|(k, n)| format!("{k} {:.2}", *n as f64 / sentences.max(1) as f64))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut top: Vec<_> = noise.iter().collect();
    top.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    eprintln!(
        "commonest other matches: {}",
        top.iter()
            .take(30)
            .map(|(k, (n, _))| format!("{k} ({n})"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut worst: Vec<_> = per_entry
        .iter()
        .filter(|(_, (t, _))| t.total() >= 3)
        .map(|(id, (t, miss))| {
            let exact = *t.0.get(&Outcome::Exact).unwrap_or(&0);
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
            "  {:3.0}%  {id:<26} {}",
            r * 100.0,
            miss.as_deref().unwrap_or("")
        );
    }
    for ((id, marked, what), (n, ex)) in &before {
        eprintln!(
            "before {id} {}: {what:<28} {n:4}  {}",
            if *marked { "marked" } else { "other " },
            ex.join("  ")
        );
    }
    if let Ok(out) = std::env::var("GRAMMAR_REPORT") {
        std::fs::write(&out, report).unwrap();
        eprintln!("\nevery miss written to {out}");
    }
    if let Ok(out) = std::env::var("GRAMMAR_NOISE") {
        let lines: String = top
            .iter()
            .map(|(k, (n, ex))| format!("{n}\t{k}\t{ex}\n"))
            .collect();
        std::fs::write(&out, lines).unwrap();
    }
}

/// Prints the breakdown of each of `BREAKDOWN=文。,文。`, for a look at
/// one sentence.
#[test]
#[ignore = "debug: BREAKDOWN=… JMDICT_BIN=… cargo test … breakdown_of -- --ignored --nocapture"]
fn breakdown_of() {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN to a full jmdict.bin");
    jmdict_core::init(&std::fs::read(&path).unwrap()).unwrap();
    let toml = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/grammar.toml");
    let compiled = compile(&std::fs::read_to_string(toml).unwrap()).unwrap();
    let dict = GrammarDict::from_bytes(&compiled.bytes).unwrap();
    let analyzer = Analyzer::new(&dict);
    let cut =
        |s: &str, start: usize, len: usize| -> String { s.chars().skip(start).take(len).collect() };
    for s in std::env::var("BREAKDOWN").unwrap_or_default().split(',') {
        let b = analyzer.analyze(s);
        let words: Vec<String> = b
            .words
            .iter()
            .map(|w| {
                let reasons: Vec<String> = w.reasons.iter().map(|r| format!("{r:?}")).collect();
                if reasons.is_empty() {
                    cut(s, w.start, w.len)
                } else {
                    format!(
                        "{}({} {})",
                        cut(s, w.start, w.len),
                        w.base,
                        reasons.join("+")
                    )
                }
            })
            .collect();
        eprintln!("\n{s}\n  words:   {}", words.join(" | "));
        for w in &b.phrases {
            let inside: Vec<String> = b
                .words
                .iter()
                .filter(|x| w.start <= x.start && x.start < w.start + w.len)
                .map(|x| cut(s, x.start, x.len))
                .collect();
            eprintln!(
                "  phrase:  {} ({}) = {}",
                cut(s, w.start, w.len),
                w.base,
                inside.join("|")
            );
        }
        for m in &b.grammar {
            let ids: Vec<&str> = m.entries.iter().map(|e| e.id.as_str()).collect();
            eprintln!(
                "  grammar: {:<14} {:?} {}",
                cut(s, m.start, m.len),
                m.kind,
                ids.join(", ")
            );
        }
    }
}
