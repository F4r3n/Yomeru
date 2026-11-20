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
            word(31, "為る", "する", vec![P::VerbSuru], "to do"),
            word(32, "以上", "いじょう", vec![P::Noun], "or more"),
            word(33, "", "が", vec![P::Particle], "subject marker; but"),
            word(34, "", "と", vec![P::Particle], "quote"),
            word(35, "", "とする", vec![P::Expression, P::VerbSuru], "to assume"),
            word(36, "中心", "ちゅうしん", vec![P::Noun], "centre"),
            word(37, "中", "じゅう", vec![P::Suffix], "throughout"),
            word(38, "雨", "あめ", vec![P::Noun], "rain"),
            word(39, "降る", "ふる", vec![P::VerbGodanRu], "to fall"),
            word(40, "雨が降る", "あめがふる", vec![P::Expression, P::VerbGodanRu], "to rain"),
        ]))
        .unwrap();
        let toml = format!(
            "next_seq = 27\n{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
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
                "homographs = [\"no-ending\"]\nafter = [\"noun-or-particle\"]"
            ),
            point(
                "no-ending",
                14,
                "の",
                "行く{の}？",
                "forms = [\"なの\"]\nhomographs = [\"no-particle\"]\nafter = [\"predicate\"]\nfinal = true"
            ),
            point("ni-suru", 15, "にする", "コーヒー{にする}。", ""),
            point("ni-shiro", 16, "にしろ", "肉{にしろ}、魚にしろ。", "continues = true"),
            point("ijou", 17, "以上", "三つ{以上}。", ""),
            point("shi", 19, "し", "高い{し}。", "after = [\"predicate\"]"),
            point("nari", 20, "なり", "見る{なり}。", "after = [\"verb-plain\"]"),
            point("garu", 21, "がる", "高{がる}。", "after = [\"adjective-stem\"]"),
            point("ijou-ni", 18, "以上に", "思った{以上に}高い。", "no_suru = true"),
            point("wa-ga", 22, "は〜が", "彼女{は本が}高い。", ""),
            point("nai-to", 23, "ないと", "食べ{ないと}。", "no_suru = true"),
            point(
                "to-shite",
                24,
                "として",
                "本{として}。",
                "after = [\"noun-or-particle\"]"
            ),
            point("to-suru", 25, "とする", "食べる{とする}。", ""),
            point("chuu-juu", 26, "中", "学校{中}。", ""),
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
fn a_na_adjectives_ni_before_naru_isnt_the_adverb() {
    // 静かになる is "become quiet": its に is なる's.
    let found = grammar("静かになる");
    assert!(
        found
            .iter()
            .all(|(_, ids, _)| !ids.contains(&"ku-adverb".to_owned())),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(t, ids, _)| t == "なる" && ids.contains(&"naru".to_owned())),
        "{found:?}"
    );
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
fn listing_shi_nari_and_garu_attach_where_they_name() {
    let has = |sentence: &str, id: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&id.to_owned()))
    };
    // し after a predicate, not する's stem after a noun.
    assert!(has("高いし", "shi"));
    assert!(!has("散歩し", "shi"));
    // なり "as soon as" after a dictionary form, not なる's stem after に.
    assert!(has("走るなり", "nari"));
    assert!(!has("本になり", "nari"));
    // がる after an adjective's stem, not after a noun's が.
    assert!(has("高がる", "garu"));
    assert!(!has("本がある", "garu"));
    // The adverb form before なる is "become": 高くなる.
    assert!(!has("高くなる", "ku-adverb"));
}

#[test]
fn a_slot_doesnt_cross_a_clause_its_examples_dont() {
    let has = |sentence: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&"shika-nai".to_owned()))
    };
    assert!(has("本しか食べない"));
    // しか〜ない's example has no 、 inside: not across one, nor a bracket.
    assert!(!has("本しか、食べない"));
    assert!(!has("本しか（食べ）ない"));
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

#[test]
fn a_sentence_ending_point_is_found_only_at_the_end() {
    let has = |sentence: &str, id: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&id.to_owned()))
    };
    // After a verb, conjugated or not, or a noun's な, and at the end.
    assert!(has("本を食べたの？", "no-ending"));
    assert!(has("走るの。", "no-ending"));
    assert!(has("本なの", "no-ending"));
    assert!(has("走るのよ", "no-ending"));
    // Not after a noun, nor before more of the sentence.
    assert!(!has("東京の", "no-ending"));
    assert!(!has("走るのは", "no-ending"));
    // にしろ that goes on is "even if"; at the end, にする's command.
    assert!(has("本にしろ、", "ni-shiro"));
    assert!(!has("本にしろ。", "ni-shiro"));
    assert!(has("本にしろ。", "ni-suru"));
    // The end is also a closing particle or a line break.
    assert!(!has("本にしろよ", "ni-shiro"));
    assert!(!has("本にしろ\n", "ni-shiro"));
    // 以上に is "more than", but before する its に is にする's.
    assert!(has("本以上に高い", "ijou-ni"));
    assert!(!has("本以上にしろ", "ijou-ni"));
    assert!(has("本以上にしろ", "ijou"));
}

#[test]
fn a_quoting_to_before_suru_isnt_a_point_of_its_own() {
    let has = |sentence: &str, id: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&id.to_owned()))
    };
    // ないと is "must", but before する (or read in として) it quotes.
    assert!(has("本を食べないと。", "nai-to"));
    assert!(!has("本を食べないとする。", "nai-to"));
    assert!(!has("本を食べないとして", "nai-to"));
    // として "as" follows a noun; after a verb it is とする's.
    assert!(has("本として", "to-shite"));
    assert!(!has("本を食べないとして", "to-shite"));
    assert!(has("本を食べないとして", "to-suru"));
}

#[test]
fn a_ga_after_a_predicate_isnt_wa_gas() {
    let has = |sentence: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&"wa-ga".to_owned()))
    };
    assert!(has("彼女は本が高い。"));
    // That が joins two clauses: "she eats it, but…".
    assert!(!has("彼女は本を食べるが、高い。"));
    assert!(!has("彼女は高いが、"));
}

#[test]
fn a_one_kanji_point_isnt_a_longer_words_first_kanji() {
    let has = |sentence: &str| {
        grammar(sentence)
            .iter()
            .any(|(_, ids, _)| ids.contains(&"chuu-juu".to_owned()))
    };
    assert!(has("学校中"));
    // 中 opens the word 中心.
    assert!(!has("東京の中心"));
}

#[test]
fn a_phrase_jmdict_lists_whole_shows_as_its_words() {
    let s = "雨が降った";
    let b = Analyzer::new(dict()).analyze(s);
    let cut = |start: usize, len: usize| -> String { s.chars().skip(start).take(len).collect() };
    let words: Vec<String> = b.words.iter().map(|w| cut(w.start, w.len)).collect();
    assert_eq!(words, ["雨", "が", "降った"]);
    let phrases: Vec<(String, String)> = b
        .phrases
        .iter()
        .map(|w| (cut(w.start, w.len), w.base.clone()))
        .collect();
    assert_eq!(phrases, [("雨が降った".to_owned(), "雨が降る".to_owned())]);
}
