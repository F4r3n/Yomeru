//! The production answer check ([`Analyzer::check_answer`]) measured, a
//! report like `grammar_breakdown.rs`:
//!
//! - `own_examples`: each point's examples and `also` sentences, which the
//!   check should accept; prints the ones it doesn't (`MISS`).
//! - `corpus_accepts`: the points the check accepts on each sentence of
//!   `ANSWER_SENTENCES` (one per line) that the breakdown doesn't find
//!   (`EXTRA`), mostly wrong accepts. `PLAIN=1` measures the plain search
//!   (`GrammarDict::check_answer`) instead. Slow: one check per point per
//!   sentence.
//!
//! ```sh
//! JMDICT_BIN=… cargo test --release -p sentence-core --test answer_vs_breakdown \
//!     own_examples -- --ignored --nocapture
//! ```
//!
//! `JMDICT_BIN` must be a `jmdict-types/full` build.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::print_stdout
)]

use std::collections::BTreeSet;

use grammar_core::{AnswerCheck, GrammarDict, JmdictVocabulary, Verdict, compile, types::plain};
use sentence_core::Analyzer;

fn load() -> GrammarDict {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN");
    jmdict_core::init(&std::fs::read(&path).unwrap()).unwrap();
    let toml = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/grammar.toml");
    let compiled = compile(&std::fs::read_to_string(toml).unwrap()).unwrap();
    GrammarDict::from_bytes(&compiled.bytes).unwrap()
}

/// Every point the check grades (not `check = "self"`), by seq.
fn graded(dict: &GrammarDict) -> Vec<String> {
    (0..100_000)
        .filter_map(|seq| dict.by_seq(seq))
        .filter(|e| e.check != AnswerCheck::SelfGraded)
        .map(|e| e.id)
        .collect()
}

#[test]
#[ignore = "report: needs JMDICT_BIN"]
fn own_examples() {
    let dict = load();
    let analyzer = Analyzer::new(&dict);
    let (mut n, mut accepted) = (0, 0);
    for id in graded(&dict) {
        let e = dict.by_id(&id).unwrap();
        for ja in e
            .examples
            .iter()
            .flat_map(|x| std::iter::once(plain(&x.ja)).chain(x.also.iter().cloned()))
        {
            n += 1;
            let v = analyzer.check_answer(&id, &ja, &JmdictVocabulary);
            if v == Verdict::Accepted {
                accepted += 1;
            } else {
                println!("MISS\t{id}\t{ja}\t{v:?}");
            }
        }
    }
    eprintln!("{accepted} of {n} examples accepted");
}

#[test]
#[ignore = "report: needs JMDICT_BIN and ANSWER_SENTENCES"]
fn corpus_accepts() {
    let dict = load();
    let analyzer = Analyzer::new(&dict);
    let ids = graded(&dict);
    let plain_search = std::env::var("PLAIN").is_ok();
    let file = std::env::var("ANSWER_SENTENCES").expect("set ANSWER_SENTENCES");
    let (mut sentences, mut found, mut extra) = (0, 0, 0);
    for s in std::fs::read_to_string(file).unwrap().lines().filter(|l| !l.is_empty()) {
        sentences += 1;
        let breakdown: BTreeSet<String> = analyzer
            .analyze(s)
            .grammar
            .into_iter()
            .flat_map(|m| m.entries.into_iter().map(|e| e.id))
            .collect();
        for id in &ids {
            let v = if plain_search {
                dict.check_answer(id, s, &JmdictVocabulary)
            } else {
                analyzer.check_answer(id, s, &JmdictVocabulary)
            };
            if v != Verdict::Accepted {
                continue;
            }
            if breakdown.contains(id) {
                found += 1;
            } else {
                extra += 1;
                println!("EXTRA\t{id}\t{s}");
            }
        }
    }
    eprintln!("{sentences} sentences: {found} accepts the breakdown finds, {extra} it doesn't");
}
