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
            word(14, "行く", "いく", vec![P::VerbGodanKuS], "to go"),
            word(15, "学校", "がっこう", vec![P::Noun], "school"),
            word(16, "散歩", "さんぽ", vec![P::Noun, P::VerbSuru], "walk"),
            word(17, "走る", "はしる", vec![P::VerbGodanRu], "to run"),
            word(18, "", "な", vec![P::Particle], "don't"),
            word(19, "", "きれい", vec![P::AdjectiveNa], "pretty"),
            word(20, "彼女", "かのじょ", vec![P::Pronoun], "she"),
            word(21, "分", "ふん", vec![P::Counter], "minute"),
            word(22, "異なる", "ことなる", vec![P::VerbGodanRu], "to differ"),
            word(23, "異", "い", vec![P::Noun], "difference"),
            word(24, "成る", "なる", vec![P::VerbGodanRu], "to become"),
            word(25, "得る", "える", vec![P::VerbIchidan], "to get"),
            // Kana only: a kana stem counts for a verb written in kana.
            word(26, "", "ある", vec![P::VerbGodanRuIrr], "to be"),
            word(27, "東京", "とうきょう", vec![P::Noun], "Tokyo"),
            word(28, "決める", "きめる", vec![P::VerbIchidan], "to decide"),
            word(29, "様", "よう", vec![P::Noun], "way"),
            word(30, "", "の", vec![P::Particle], "of"),
        ]))
        .unwrap();
        let toml = format!(
            "next_seq = 14\n{}{}{}{}{}{}{}{}{}{}{}{}{}",
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
            // Points found only after a kind of word.
            point(
                "ni-iku",
                7,
                "に行く",
                "食べ{に行く}。",
                "after = [\"masu-stem\", \"suru-noun\"]"
            ),
            point(
                "na-prohibition",
                8,
                "な",
                "走る{な}。",
                "after = [\"verb-plain\"]"
            ),
            point("number-wa", 9, "は", "三十分{は}。", "after = [\"amount\"]"),
            point("naru", 10, "なる", "大きく{なる}。", ""),
            point(
                "uru",
                11,
                "得る",
                "あり{得る}。",
                "forms = [\"あり得る\"]\nafter = [\"masu-stem\"]"
            ),
            point(
                "no-you-na",
                12,
                "のような",
                "東京{のような}町。",
                "forms = [\"ような\"]\nafter = [\"noun-or-demonstrative\"]"
            ),
            point(
                "no-particle",
                13,
                "の",
                "東京{の}町。",
                "after = [\"noun-or-particle\"]"
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

#[test]
fn a_point_is_found_only_after_what_it_names() {
    let has = |sentence: &str, id: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&id.to_owned()))
    };
    // A ます-stem or a する noun, not a place.
    assert!(has("食べに行く", "ni-iku"));
    assert!(has("散歩に行く", "ni-iku"));
    assert!(!has("学校に行く", "ni-iku"));
    // A verb in its dictionary form, not a な adjective.
    assert!(has("走るな", "na-prohibition"));
    assert!(!has("きれいな本", "na-prohibition"));
    // An amount: digits, or a counter after a numeral; not a pronoun.
    assert!(has("３０分は", "number-wa"));
    assert!(has("三十分は", "number-wa"));
    assert!(!has("彼女は", "number-wa"));
}

#[test]
fn a_verb_of_one_piece_isnt_split_for_grammar() {
    // 異なる isn't 異 + なる.
    let found = grammar("異なる");
    assert!(
        found
            .iter()
            .all(|(_, ids, _)| !ids.contains(&"naru".to_owned())),
        "{found:?}"
    );
}

#[test]
fn after_reads_the_stem_a_form_holds() {
    let has = |sentence: &str, id: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&id.to_owned()))
    };
    // あり得る holds its stem; 得た after を has none.
    assert!(has("あり得る", "uru"));
    assert!(!has("本を得た", "uru"));
    // ような after a noun's の, not after a verb (よう is a noun too).
    assert!(has("東京のような", "no-you-na"));
    assert!(!has("決めるような", "no-you-na"));
    // の linking nouns, not the の that explains or asks after a verb.
    assert!(has("東京の本", "no-particle"));
    assert!(!has("得たの", "no-particle"));
}
