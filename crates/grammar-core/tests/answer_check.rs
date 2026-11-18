//! Every reference example of `data/grammar.toml`, and each of its `also`
//! sentences, passes the answer check of its own point: an example the check can't find its point in is
//! either a gap of the check or a weak example.
//!
//! The full run needs JMdict for the verbs and the word boundaries, so it
//! is ignored:
//!
//! ```sh
//! JMDICT_BIN=$PWD/extension/data/jmdict.bin cargo test --release -p grammar-core \
//!     --test answer_check -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::panic, clippy::print_stderr)]

use grammar_core::{
    AnswerCheck, GrammarDict, JmdictVocabulary, Verdict, Vocabulary, compile, types::plain,
};

fn dict() -> GrammarDict {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/grammar.toml");
    let compiled = compile(&std::fs::read_to_string(path).unwrap()).unwrap();
    GrammarDict::from_bytes(&compiled.bytes).unwrap()
}

/// The examples whose point the check misses: `(id, sentence, verdict)`.
fn misses(
    dict: &GrammarDict,
    vocab: &impl Vocabulary,
    keep: impl Fn(&AnswerCheck) -> bool,
) -> (usize, Vec<(String, String, Verdict)>) {
    let mut checked = 0;
    let mut out = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    // Seqs have gaps (removed points) but stay under next_seq; every
    // point must be reached, or the bound is too low.
    for seq in 0..100_000 {
        if let Some(e) = dict.by_seq(seq) {
            ids.push(e.id);
        }
    }
    assert_eq!(ids.len(), dict.len(), "some points weren't reached by seq");
    for id in ids {
        let e = dict.by_id(&id).unwrap();
        if !keep(&e.check) {
            continue;
        }
        // The `also` sentences are correct answers too.
        for ja in e
            .examples
            .iter()
            .flat_map(|ex| std::iter::once(plain(&ex.ja)).chain(ex.also.iter().cloned()))
        {
            checked += 1;
            let v = dict.check_answer(&e.id, &ja, vocab);
            if v != Verdict::Accepted && v != Verdict::SelfGraded {
                out.push((e.id.clone(), ja, v));
            }
        }
    }
    (checked, out)
}

/// Misses known and accepted:
/// - the past かった is no form of its own: it would match inside わかった
///   and なかった;
/// - a particle followed by a kana word (はか, もし) reads as part of it.
const KNOWN: &[(&str, &str)] = &[
    ("i-adjective-forms", "とても楽しかったです。"),
    ("number-mo", "新しい自転車は５００００円もした。"),
    ("number-wa", "駅まで三十分はかかる。"),
];

/// Prints every miss and fails on one not in [`KNOWN`].
fn report(checked: usize, misses: &[(String, String, Verdict)]) {
    for (id, ja, v) in misses {
        eprintln!("{id}\t{ja}\t{v:?}");
    }
    eprintln!("{} of {checked} examples missed", misses.len());
    let new: Vec<_> = misses
        .iter()
        .filter(|(id, ja, _)| !KNOWN.contains(&(id.as_str(), ja.as_str())))
        .collect();
    assert!(new.is_empty(), "new misses: {new:#?}");
}

/// No dictionary: no word is longer than a char, nothing is a verb.
struct NoWords;
impl Vocabulary for NoWords {
    fn is_verb(&self, _: &str) -> bool {
        false
    }
    fn word_len_at(&self, _: &str, _: usize) -> usize {
        1
    }
}

#[test]
fn forms_points_find_their_own_examples() {
    let dict = dict();
    let (checked, m) = misses(&dict, &NoWords, |c| *c == AnswerCheck::Forms);
    report(checked, &m);
    assert!(checked > 4000, "{checked}");
}

#[test]
#[ignore = "needs JMDICT_BIN"]
fn every_point_finds_its_own_examples_with_jmdict() {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN");
    jmdict_core::init(&std::fs::read(&path).unwrap()).unwrap();
    let dict = dict();
    let (checked, m) = misses(&dict, &JmdictVocabulary, |_| true);
    report(checked, &m);
}
