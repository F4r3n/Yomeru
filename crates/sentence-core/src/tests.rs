// Fixtures: a malformed one is a bug in the test, so failing loudly is right.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::OnceLock;

use grammar_core::GrammarDict;
use jmdict_core::test_utils::{build_binary, word};
use jmdict_types::PartOfSpeech as P;

use crate::{Analyzer, MatchKind};

fn dict() -> &'static GrammarDict {
    static GRAMMAR: OnceLock<GrammarDict> = OnceLock::new();
    GRAMMAR.get_or_init(|| {
        jmdict_core::init(&build_binary(&[
            word(1, "本", "ほん", vec![P::Noun], "book"),
            word(2, "食べる", "たべる", vec![P::VerbIchidan], "to eat"),
            word(3, "", "を", vec![P::Particle], "object marker"),
            word(4, "", "しか", vec![P::Particle], "only"),
            word(5, "", "ない", vec![P::AuxiliaryAdjective], "not"),
            word(6, "", "いい", vec![P::Adjective], "good"),
            word(7, "彼ら", "かれら", vec![P::Pronoun], "they"),
            word(8, "", "は", vec![P::Particle], "topic marker"),
            word(9, "高い", "たかい", vec![P::Adjective], "high"),
            word(10, "高さ", "たかさ", vec![P::Noun], "height"),
            word(11, "静か", "しずか", vec![P::AdjectiveNa], "quiet"),
            word(12, "", "に", vec![P::Particle], "in, to"),
            word(13, "", "ける", vec![P::VerbIchidan], "to kick"),
        ]))
        .unwrap();
        let toml = format!(
            "next_seq = 7\n{}{}{}{}{}{}",
            point("te-mo-ii", 1, "てもいい", "食べ{てもいい}。", ""),
            point("tachi", 2, "ら", "彼{ら}。", ""),
            point("shika-nai", 3, "しか〜ない", "本{しか食べない}。", ""),
            // A pattern no text holds: only the word's deinflection finds
            // it.
            point(
                "causative",
                4,
                "使役形",
                "食べ{させる}。",
                "check = { inflection = [\"causative\"] }"
            ),
            // Points read off a word's form.
            point("ku-adverb", 5, "く", "{高く}。", "reads = [\"adverbial\"]"),
            point(
                "sa-noun",
                6,
                "さ",
                "{高さ}。",
                "reads = [\"nominalization\"]"
            ),
        );
        GrammarDict::from_bytes(&grammar_core::compile(&toml).unwrap().bytes).unwrap()
    })
}

fn point(id: &str, seq: u32, pattern: &str, example: &str, extra: &str) -> String {
    format!(
        r#"
[[point]]
id = "{id}"
seq = {seq}
pattern = "{pattern}"
jlpt = "N4"
meaning = "meaning of {id}"
formation = ["Verb + {pattern}"]
explanation = "What {id} does."
examples = [{{ source = "yomeru", id = "y1", ja = "{example}", en = "An example." }}]
{extra}
"#
    )
}

/// `(text, ids, kind)` of each grammar match in `sentence`.
fn grammar(sentence: &str) -> Vec<(String, Vec<String>, MatchKind)> {
    let analyzer = Analyzer::new(dict());
    analyzer
        .analyze(sentence)
        .grammar
        .into_iter()
        .map(|m| {
            let text: String = sentence.chars().skip(m.start).take(m.len).collect();
            (text, m.entries.into_iter().map(|e| e.id).collect(), m.kind)
        })
        .collect()
}

#[test]
fn a_pattern_starts_in_a_conjugated_ending() {
    let found = grammar("本を食べてもいい。");
    assert!(
        found
            .iter()
            .any(|(t, ids, k)| t == "てもいい" && ids == &["te-mo-ii"] && *k == MatchKind::Text),
        "{found:?}"
    );
}

#[test]
fn a_pattern_doesnt_start_inside_a_word() {
    // ら is the last char of the word 彼ら, not the suffix.
    let found = grammar("彼らは本を食べない。");
    assert!(
        !found
            .iter()
            .any(|(_, ids, _)| ids.contains(&"tachi".to_owned())),
        "{found:?}"
    );
}

#[test]
fn slot_pieces_are_found_on_word_edges_with_words_between() {
    let analyzer = Analyzer::new(dict());
    let b = analyzer.analyze("本しか食べない。");
    let m = b
        .grammar
        .iter()
        .find(|m| m.kind == MatchKind::Slots)
        .expect("しか〜ない");
    assert_eq!(m.entries[0].id, "shika-nai");
    // しか at 1, ない (the ending of 食べない) at 5.
    assert_eq!(m.pieces, [(1, 2), (5, 2)]);
    assert_eq!((m.start, m.len), (1, 6));
    // A slot holds at least one word.
    assert!(
        !grammar("本しかない。")
            .iter()
            .any(|(_, ids, _)| ids.contains(&"shika-nai".to_owned()))
    );
}

#[test]
fn a_conjugation_point_comes_from_the_word() {
    let found = grammar("本を食べさせた。");
    assert!(
        found.iter().any(|(t, ids, k)| t == "させた"
            && ids == &["causative"]
            && *k == MatchKind::Conjugation),
        "{found:?}"
    );
}

#[test]
fn a_point_is_read_off_a_words_form() {
    // 高く: the lattice's own deinflection; 高さ: an entry of its own, read
    // by its head; 静かに: a な adjective and に.
    for (sentence, text, id) in [
        ("高く", "高く", "ku-adverb"),
        ("高さ", "高さ", "sa-noun"),
        ("静かに", "静かに", "ku-adverb"),
    ] {
        let found = grammar(sentence);
        assert!(
            found.contains(&(text.to_owned(), vec![id.to_owned()], MatchKind::Conjugation)),
            "{sentence}: {found:?}"
        );
    }
}

#[test]
fn a_read_point_isnt_found_by_its_pattern() {
    // く as an ending is in けた (the past of a would-be 蹴く), and ける
    // is no adjective: nothing reads as the く adverb.
    let found = grammar("けた");
    assert!(
        found
            .iter()
            .all(|(_, ids, _)| !ids.contains(&"ku-adverb".to_owned())),
        "{found:?}"
    );
}
