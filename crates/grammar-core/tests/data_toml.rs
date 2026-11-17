//! Compiles the checked-in `data/grammar.toml` and looks a few points up,
//! so a broken entry fails `cargo test` rather than the build.

#![allow(clippy::unwrap_used, clippy::print_stderr)]

use grammar_core::{GrammarDict, compile};

#[test]
fn the_checked_in_dictionary_compiles_and_answers() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/grammar.toml");
    let compiled = compile(&std::fs::read_to_string(path).unwrap()).unwrap();
    for w in &compiled.warnings {
        eprintln!("warning: {w}");
    }
    let dict = GrammarDict::from_bytes(&compiled.bytes).unwrap();
    assert_eq!(dict.len(), compiled.entries);
    assert!(dict.len() > 900, "{} points", dict.len());

    let te_mo_ii = dict.by_id("te-mo-ii").unwrap();
    assert_eq!(te_mo_ii.pattern, "てもいい");
    // The number cards key on leads back to the point. These are fixed
    // for good: a change re-keys every card of these points.
    assert_eq!(te_mo_ii.seq, 1);
    assert_eq!(dict.by_seq(1).unwrap().id, "te-mo-ii");
    assert!(dict.by_seq(0).is_none());
    assert!(dict.lookup("てもいい").iter().any(|e| e.id == "te-mo-ii"));
    let (entries, len) = dict.hover_match("てもいいですか。", 24).unwrap();
    assert!(entries.iter().any(|e| e.id == "te-mo-ii"), "{entries:?}");
    assert!(len >= 4);
    eprintln!(
        "{} points, {} keys, {} KB",
        dict.len(),
        dict.key_count(),
        compiled.bytes.len() / 1024
    );
}
