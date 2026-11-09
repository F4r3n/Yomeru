//! The lookup against its plain reference on the full dictionary and real
//! sentences. Too slow and too dependent on local files for the normal
//! suite, so ignored; run it after changing the lookup:
//!
//! ```sh
//! JMDICT_BIN=path/to/jmdict.bin cargo test --release -p jmdict-core \
//!     --test real_dict -- --ignored
//! ```
//!
//! `JMDICT_BIN` must be built with `jmdict-types/full` (tests enable it
//! through the dev-dependency), or decoding fails at init. Sentences come
//! from `examples.utf` (Tanaka corpus) at the repo root, or `EXAMPLES_UTF`.

// Test-only code: the workspace's production lints don't apply.
#![allow(clippy::panic, clippy::print_stderr)]

mod reference;

use std::collections::HashSet;

fn sequences(key: &str) -> Vec<u32> {
    jmdict_core::lookup(key)
        .iter()
        .map(|e| e.sequence.to_native())
        .collect()
}

/// Every `step`-th Japanese sentence (`A:` lines) of the corpus.
fn sentences(step: usize) -> Vec<String> {
    let path = std::env::var("EXAMPLES_UTF")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples.utf").into());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("can't read {path} (set EXAMPLES_UTF): {e}"));
    text.lines()
        .filter_map(|l| l.strip_prefix("A: "))
        .map(|l| l.split('\t').next().unwrap_or_default().to_owned())
        .step_by(step)
        .collect()
}

#[test]
#[ignore = "needs JMDICT_BIN (a `full` build) and examples.utf"]
fn same_answers_as_the_reference_on_real_text() {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN to a full jmdict.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("can't read {path}: {e}"));
    jmdict_core::init(&bytes).expect("init: is JMDICT_BIN a `full` build?");

    let sentences = sentences(10);
    assert!(sentences.len() > 10_000, "only {} sentences", sentences.len());
    let mut positions = 0;
    for s in &sentences {
        for (i, _) in s.char_indices() {
            let rest = &s[i..];
            let fast = jmdict_core::lookup_longest_match(rest, 20)
                .map(|(es, n)| (es.iter().map(|e| e.sequence.to_native()).collect(), n));
            assert_eq!(
                fast,
                reference::longest_match(rest, 20, &sequences),
                "lookup_longest_match({rest:?})"
            );
            positions += 1;
        }
    }

    // The highlighter's scan over one long page, with every word it meets
    // known, so each match is reported.
    let page = sentences.join("\n");
    let known: HashSet<u32> = sentences
        .iter()
        .flat_map(|s| s.char_indices().map(move |(i, _)| &s[i..]))
        .filter_map(|rest| jmdict_core::lookup_longest_match(rest, 20))
        .flat_map(|(es, _)| es.into_iter().map(|e| e.sequence.to_native()))
        .collect();
    let hits = jmdict_core::find_in_text(&page, &known);
    assert_eq!(hits, reference::find_in_text(&page, &known, &sequences));
    eprintln!(
        "{} sentences, {positions} positions, {} highlights: all as the reference",
        sentences.len(),
        hits.len()
    );
}
