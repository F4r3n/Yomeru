//! Hand-written entries in the shape of `data/grammar.toml`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use crate::types::{Span, parse_marked, plain};
use crate::{AnswerCheck, GrammarDict, Inflection, compile};

/// A `grammar.toml` of `points`: each `seq = @` numbered in order from 1,
/// and `next_seq` above them all.
fn numbered(points: &str) -> String {
    let mut out = String::from("next_seq = 1000\n");
    for (i, part) in points.split("seq = @").enumerate() {
        if i > 0 {
            out.push_str(&format!("seq = {i}"));
        }
        out.push_str(part);
    }
    out
}

/// One `[[point]]`, the fields the tests vary given, the rest filled in;
/// [`numbered`] gives it its `seq`.
fn point(id: &str, pattern: &str, forms: &[&str], extra: &str) -> String {
    let forms: Vec<String> = forms.iter().map(|f| format!("{f:?}")).collect();
    format!(
        r#"
[[point]]
id = "{id}"
seq = @
pattern = "{pattern}"
forms = [{forms}]
jlpt = "N4"
meaning = "meaning of {id}"
formation = ["Verb［る］ + {pattern}"]
explanation = """
What {id} does. \
Second line."""
examples = [
  {{ tatoeba = "1_2", ja = "前{{{pattern}}}後。", en = "An example." }},
  {{ source = "yomeru", id = "y1", ja = "{{{pattern}}}だ。", en = "Ours." }},
]
{extra}
"#,
        forms = forms.join(", ")
    )
}

fn sample() -> String {
    let points = [
        point(
            "you-ni-suru",
            "ようにする",
            &["ようにします"],
            r#"related = ["you-ni-naru", "not-written"]"#,
        ),
        point("you-ni-naru", "ようになる", &[], ""),
        point("you-ni", "ように", &[], ""),
        point("te-yokatta", "てよかった", &[], "")
            .replace("en = \"Ours.\"", "en = \"Ours.\", also = [\"よかったです。\"]"),
        point("yokatta", "よかった", &[], ""),
        point("tte", "って", &[], "hover = false\ncheck = \"self\""),
        point("wa-particle", "は", &[], ""),
        point(
            "causative",
            "させる",
            &[],
            r#"check = { inflection = ["causative", "causative-passive"] }"#,
        ),
        point("shidai", "次第", &[], r#"homographs = ["shidai-de"]"#),
        point(
            "shidai-de",
            "次第で",
            &["次第"],
            r#"homographs = ["shidai"]"#,
        ),
    ]
    .concat();
    numbered(&points)
}

fn sample_dict() -> GrammarDict {
    GrammarDict::from_bytes(&compile(&sample()).unwrap().bytes).unwrap()
}

fn ids(entries: &[crate::GrammarEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.id.as_str()).collect()
}

#[test]
fn compiles_every_field() {
    let compiled = compile(&sample()).unwrap();
    assert_eq!(compiled.entries, 10);
    assert_eq!(
        compiled.warnings,
        ["you-ni-suru: related not-written isn't written yet"]
    );
    let dict = GrammarDict::from_bytes(&compiled.bytes).unwrap();
    assert_eq!(dict.len(), 10);
    let e = dict.by_id("you-ni-suru").unwrap();
    assert_eq!(e.pattern, "ようにする");
    assert_eq!(e.forms, ["ようにします"]);
    assert_eq!(e.jlpt, "N4");
    assert_eq!(e.explanation, "What you-ni-suru does. Second line.");
    assert_eq!(e.notes, "");
    assert!(e.hover);
    assert_eq!(e.related.len(), 1);
    assert_eq!(e.related[0].id, "you-ni-naru");
    assert_eq!(e.related[0].pattern, "ようになる");
    assert_eq!(e.examples.len(), 2);
    assert_eq!(
        e.examples[0].ja,
        [
            Span::Text { text: "前".into() },
            Span::Highlight {
                text: "ようにする".into()
            },
            Span::Text {
                text: "後。".into()
            },
        ]
    );
    assert_eq!(e.examples[0].tatoeba, "1_2");
    assert_eq!(e.examples[1].tatoeba, "");
    assert_eq!(e.examples[0].key, "1_2");
    assert_eq!(e.examples[1].key, "y1");
    assert!(e.examples[1].also.is_empty());
    assert_eq!(
        dict.by_id("te-yokatta").unwrap().examples[1].also,
        ["よかったです。"]
    );
    assert!(!dict.by_id("tte").unwrap().hover);
    assert_eq!(e.check, AnswerCheck::Forms);
    assert_eq!(dict.by_id("tte").unwrap().check, AnswerCheck::SelfGraded);
    assert_eq!(
        dict.by_id("causative").unwrap().check,
        AnswerCheck::Inflection {
            forms: vec![Inflection::Causative, Inflection::CausativePassive]
        }
    );
    assert!(dict.by_id("missing").is_none());
    // Numbered in file order: you-ni-suru is the first.
    assert_eq!(e.seq, 1);
    assert_eq!(dict.by_seq(1).unwrap().id, "you-ni-suru");
    assert_eq!(dict.by_seq(3).unwrap().id, "you-ni");
    assert!(dict.by_seq(999).is_none());
}

#[test]
fn parses_one_mark_per_sentence() {
    assert_eq!(plain(&parse_marked("a{b}c").unwrap()), "abc");
    assert_eq!(parse_marked("{b}").unwrap().len(), 1);
    for bad in ["abc", "a{}c", "{a}{b}", "a{b", "a}b{c", "a{b{c}}"] {
        assert!(parse_marked(bad).is_none(), "{bad}");
    }
}

#[test]
fn looks_up_by_pattern_and_by_form() {
    let dict = sample_dict();
    assert_eq!(ids(&dict.lookup("ようにする")), ["you-ni-suru"]);
    assert_eq!(ids(&dict.lookup("ようにします")), ["you-ni-suru"]);
    assert!(dict.lookup("ようにし").is_empty());
}

#[test]
fn a_lookup_brings_the_homographs_along() {
    let dict = sample_dict();
    // 次第 is both entries' spelling; 次第で only the second's.
    assert_eq!(ids(&dict.lookup("次第")), ["shidai", "shidai-de"]);
    assert_eq!(ids(&dict.lookup("次第で")), ["shidai-de", "shidai"]);
}

#[test]
fn longest_match_prefers_the_longer_pattern_and_deinflects() {
    let dict = sample_dict();
    let (entries, len) = dict.lookup_longest_match("ようにするつもり", 24).unwrap();
    assert_eq!((ids(&entries), len), (vec!["you-ni-suru"], 5));
    let (entries, len) = dict.lookup_longest_match("ようにしなかった。", 24).unwrap();
    assert_eq!((ids(&entries), len), (vec!["you-ni-suru"], 8));
    let (entries, len) = dict.lookup_longest_match("ように", 24).unwrap();
    assert_eq!((ids(&entries), len), (vec!["you-ni"], 3));
    assert!(dict.lookup_longest_match("ようにする", 2).is_none());
    assert!(dict.lookup_longest_match("全然", 24).is_none());
}

#[test]
fn hover_skips_search_only_and_one_char_points() {
    let dict = sample_dict();
    assert_eq!(
        ids(&dict.lookup_longest_match("って", 24).unwrap().0),
        ["tte"]
    );
    assert!(dict.hover_match("って", 24).is_none());
    assert_eq!(
        ids(&dict.lookup_longest_match("は", 24).unwrap().0),
        ["wa-particle"]
    );
    assert!(dict.hover_match("は", 24).is_none());
    let (entries, len) = dict.hover_match("ようにする", 24).unwrap();
    assert_eq!((ids(&entries), len), (vec!["you-ni-suru"], 5));
}

/// お〜 patterns match the word their 〜 stands for, conjugated too, but not
/// a noun or an adjective after お / ご.
#[test]
fn hover_fills_the_gap_of_honorific_patterns() {
    let src = [
        point("o-da", "お〜だ", &["お〜です", "ご〜です"], ""),
        point("o-ni-naru", "お〜になる", &["ご〜になる"], ""),
        point("o-kudasai", "お〜ください", &["ご〜ください"], ""),
        point("ni-naru", "になる", &[], ""),
    ]
    .concat();
    let dict = GrammarDict::from_bytes(&compile(&numbered(&src)).unwrap().bytes).unwrap();
    let hover = |t| dict.hover_match(t, 24).map(|(e, n)| (ids(&e).join(","), n));
    assert_eq!(hover("お待ちですか"), Some(("o-da".into(), 5)));
    assert_eq!(hover("お出かけですか"), Some(("o-da".into(), 6)));
    assert_eq!(hover("お帰りになりました"), Some(("o-ni-naru".into(), 9)));
    assert_eq!(hover("ご利用ください"), Some(("o-kudasai".into(), 7)));
    for t in [
        "お茶です",
        "おいしいです",
        "お元気です",
        "ご家族です",
        "お金になる",
    ] {
        assert_eq!(hover(t), None, "{t}");
    }
    // Capped like any match: the whole お帰りになる doesn't fit in 4 chars.
    assert_eq!(dict.hover_match("お帰りになる", 4), None);
}

#[test]
fn search_puts_exact_then_conjugated_then_inner_then_prefix_hits() {
    let dict = sample_dict();
    assert_eq!(ids(&dict.search("ようにしなかった", 10)), ["you-ni-suru"]);
    // てよかった inside the text; よかった lies within it.
    assert_eq!(ids(&dict.search("来てよかった", 10)), ["te-yokatta"]);
    assert_eq!(
        ids(&dict.search("ように", 10)),
        ["you-ni", "you-ni-suru", "you-ni-naru"]
    );
    // Search finds what hover leaves out.
    assert_eq!(ids(&dict.search("って", 10)), ["tte"]);
    assert_eq!(dict.search("ように", 1).len(), 1);
    assert!(dict.search("  ", 10).is_empty());
}

#[test]
fn prefix_search_lists_each_entry_once() {
    let dict = sample_dict();
    // ようにする is found by its pattern and its form: listed once.
    assert_eq!(ids(&dict.lookup_prefix("ようにす", 10)), ["you-ni-suru"]);
}

#[test]
fn refuses_what_a_lookup_would_get_wrong() {
    let base = point("a", "あい", &[], "");
    let cases = [
        (format!("{base}{base}"), "duplicate id"),
        (point("A b", "あい", &[], ""), "slug"),
        (base.replace("\"N4\"", "\"N6\""), "jlpt"),
        (base.replace("前{あい}後", "前あい後"), "mark the pattern"),
        (base.replace("tatoeba = \"1_2\", ", ""), "either `tatoeba`"),
        (base.replace("id = \"y1\", ", ""), "needs id"),
        (base.replace("\"y1\"", "\"y01\""), "must be y1"),
        (base.replace("\"y1\"", "\"1\""), "must be y1"),
        (
            base.replace("tatoeba = \"1_2\", ", "tatoeba = \"1_2\", id = \"y2\", "),
            "only our own",
        ),
        (
            base.replace("examples = [", "examples = [\n  { source = \"yomeru\", id = \"y1\", ja = \"{あい}。\", en = \"Again.\" },"),
            "keyed y1",
        ),
        (
            base.replace("examples = [", "examples = [\n  { tatoeba = \"1_2\", ja = \"{あい}。\", en = \"Again.\" },"),
            "keyed 1_2",
        ),
        (
            format!("{base}{}", point("b", "うえ", &["あい"], "")),
            "both spell",
        ),
        (
            point("c", "うえ", &[], r#"homographs = ["a"]"#),
            "not an entry",
        ),
        (
            format!("{base}{}", point("c", "あい", &[], r#"homographs = ["a"]"#)),
            "doesn't name it back",
        ),
        (format!("{base}\nunknown = 1"), "parsing"),
        (format!("{base}\ncheck = \"none\""), "write \"self\""),
        (format!("{base}\ncheck = {{ inflection = [] }}"), "names no form"),
        (format!("{base}\ncheck = {{ inflection = [\"passiv\"] }}"), "unknown inflection"),
        (base.replace("en = \"Ours.\"", "en = \"Ours.\", also = [\"{あい}よ。\"]"), "plain, no"),
        (base.replace("en = \"Ours.\"", "en = \"Ours.\", also = [\" \"]"), "empty `also`"),
        (base.replace("en = \"Ours.\"", "en = \"Ours.\", also = [\"あい だ。\"]"), "again"),
        (base.replace("en = \"Ours.\"", "en = \"Ours.\", also = [\"あいよ。\", \"あいよ。\"]"), "again"),
    ];
    for (toml, want) in cases {
        let err = format!("{:#}", compile(&numbered(&toml)).err().unwrap());
        assert!(err.contains(want), "{want}: {err}");
    }
    // The numbers: given by hand here, [`numbered`] would fix them.
    let a = point("a", "あい", &[], "");
    let b = point("b", "うえ", &[], "");
    let cases = [
        (
            format!(
                "next_seq = 5\n{}{}",
                a.replace("@", "1"),
                b.replace("@", "1")
            ),
            "already a's",
        ),
        (format!("next_seq = 5\n{}", a.replace("@", "5")), "next_seq"),
        (format!("next_seq = 5\n{}", a.replace("@", "0")), "from 1"),
        (a.replace("@", "1"), "next_seq"),
        (
            format!("next_seq = 5\n{}", a.replace("seq = @\n", "")),
            "seq",
        ),
    ];
    for (toml, want) in cases {
        let err = format!("{:#}", compile(&toml).err().unwrap());
        assert!(err.contains(want), "{want}: {err}");
    }
    // A gap is fine: that number was a removed entry's.
    assert!(compile(&format!("next_seq = 9\n{}", a.replace("@", "7"))).is_ok());
}

#[test]
fn refuses_bad_or_other_version_bytes() {
    let mut bytes = compile(&sample()).unwrap().bytes;
    assert!(GrammarDict::from_bytes(b"nope").is_err());
    bytes[4] = crate::FORMAT_VERSION + 1;
    let err = GrammarDict::from_bytes(&bytes).err().unwrap().to_string();
    assert!(err.contains("rebuild"), "{err}");
    bytes[4] = crate::FORMAT_VERSION;
    bytes.truncate(bytes.len() - 3);
    assert!(GrammarDict::from_bytes(&bytes).is_err());
}

mod answers {
    use super::{numbered, point};
    use crate::{GrammarDict, Related, Verdict, Vocabulary, compile};

    /// A dictionary of a few words: `verbs` in dictionary form, `words`
    /// for the word boundaries.
    struct Words {
        verbs: &'static [&'static str],
        words: &'static [&'static str],
    }

    impl Vocabulary for Words {
        fn is_verb(&self, base: &str) -> bool {
            self.verbs.contains(&base)
        }
        fn word_len_at(&self, text: &str, char_offset: usize) -> usize {
            let rest: String = text.chars().skip(char_offset).collect();
            self.words
                .iter()
                .filter(|w| rest.starts_with(**w))
                .map(|w| w.chars().count())
                .max()
                .unwrap_or(1)
        }
    }

    const NONE: Words = Words {
        verbs: &[],
        words: &[],
    };

    fn dict() -> GrammarDict {
        let points = [
            point("te-kara", "てから", &[], r#"related = ["ta-ato-de"]"#),
            point("ta-ato-de", "たあとで", &[], ""),
            point("te-kudasai", "てください", &[], ""),
            point("te-shimau", "てしまう", &[], ""),
            point("koto-ga-aru", "ることがある", &[], ""),
            point("you-to-omou", "ようと思う", &[], ""),
            point("shika-nai", "しか〜ない", &[], ""),
            point("koto-wa-nai", "ことはない", &[], ""),
            point("ta-hyoushi-ni", "た拍子に", &[], ""),
            point("kangaerarenai", "考えられない", &[], ""),
            point("to-wa-omowanakatta", "とは思わなかった", &[], ""),
            point("touzen-da", "当然だ", &[], ""),
            point("totemo-nai", "とても〜ない", &[], ""),
            point("you-to-shinai", "ようとしない", &[], ""),
            point("n-datta", "んだった", &[], ""),
            point("sa-particle", "さ", &[], ""),
            point("yo", "よ", &[], ""),
            point("yone", "よね", &[], ""),
            point("tte", "って", &[], "check = \"self\""),
            point(
                "causative",
                "させる",
                &[],
                r#"check = { inflection = ["causative"] }"#,
            ),
            point(
                "imperative",
                "命令形",
                &[],
                r#"check = { inflection = ["imperative"] }"#,
            ),
            point(
                "causative-passive",
                "させられる",
                &[],
                r#"check = { inflection = ["causative-passive"] }"#,
            ),
        ]
        .concat();
        GrammarDict::from_bytes(&compile(&numbered(&points)).unwrap().bytes).unwrap()
    }

    fn related(id: &str, pattern: &str) -> Related {
        Related {
            id: id.into(),
            pattern: pattern.into(),
        }
    }

    #[test]
    fn the_point_as_written_or_conjugated_is_accepted() {
        let d = dict();
        for (id, answer) in [
            ("te-kara", "ご飯を食べてから、出かけます。"),
            ("te-kara", "ご飯を 食べて から 出かけます"),
            ("te-shimau", "全部食べてしまった。"),
            // A leading る stands for any dictionary form.
            ("koto-ga-aru", "たまに手紙を書くことがある。"),
            ("koto-ga-aru", "たまに見ることがあります。"),
            // A leading よう, for the godan volitional too.
            ("you-to-omou", "帰ろうと思います。"),
            ("you-to-omou", "寝ようと思う。"),
            // A 〜 splits the pattern into pieces found in order.
            ("shika-nai", "水しか飲まない。"),
            // た for だ.
            ("ta-hyoushi-ni", "転んだ拍子に眼鏡が落ちた。"),
            // ありません read as ない.
            ("koto-wa-nai", "心配することはありません。"),
            // The polite form of the pattern's own conjugation.
            ("kangaerarenai", "そんなことは考えられません。"),
            ("to-wa-omowanakatta", "難しいとは思いませんでした。"),
            // The polite copula for the plain one.
            ("touzen-da", "怒るのは当然です。"),
            ("n-datta", "傘を持ってくるんでした。"),
            ("totemo-nai", "とても買えません。"),
            ("you-to-shinai", "誰の話も聞こうとしません。"),
        ] {
            assert_eq!(
                d.check_answer(id, answer, &NONE),
                Verdict::Accepted,
                "{id}: {answer}"
            );
        }
    }

    #[test]
    fn a_related_point_is_close_and_others_are_named() {
        let d = dict();
        assert_eq!(
            d.check_answer("te-kara", "食べたあとで出かけます。", &NONE),
            Verdict::Close {
                related: related("ta-ato-de", "たあとで")
            }
        );
        assert_eq!(
            d.check_answer("te-kara", "食べてください。", &NONE),
            Verdict::Other {
                found: vec![related("te-kudasai", "てください")]
            }
        );
        assert_eq!(
            d.check_answer("te-kara", "出かけます。", &NONE),
            Verdict::NotFound
        );
        // In order, not anywhere: ない before しか isn't the pattern.
        assert_eq!(
            d.check_answer("shika-nai", "ないしか", &NONE),
            Verdict::NotFound
        );
    }

    #[test]
    fn self_graded_and_unknown_points_are_not_checked() {
        let d = dict();
        assert_eq!(d.check_answer("tte", "何でも", &NONE), Verdict::SelfGraded);
        assert_eq!(d.check_answer("gone", "何でも", &NONE), Verdict::SelfGraded);
    }

    #[test]
    fn inflections_need_a_known_verb() {
        let d = dict();
        let words = Words {
            verbs: &["書く", "待つ", "舞う"],
            words: &[],
        };
        assert_eq!(
            d.check_answer("causative", "子供に手紙を書かせた。", &words),
            Verdict::Accepted
        );
        assert_eq!(
            d.check_answer("causative-passive", "一時間も待たされた。", &words),
            Verdict::Accepted
        );
        // 書かせる is causative but not causative-passive.
        assert_ne!(
            d.check_answer("causative-passive", "書かせた。", &words),
            Verdict::Accepted
        );
        assert_eq!(
            d.check_answer("imperative", "早く書け！", &words),
            Verdict::Accepted
        );
        // まえ → まう is the imperative rule: it counts only when the
        // vocabulary knows まう as a verb.
        assert_eq!(
            d.check_answer("imperative", "駅のまえで", &words),
            Verdict::NotFound
        );
        let mau = Words {
            verbs: &["まう"],
            words: &[],
        };
        assert_eq!(
            d.check_answer("imperative", "駅のまえで", &mau),
            Verdict::Accepted
        );
    }

    #[test]
    fn a_one_char_kana_point_is_not_found_inside_a_kana_word() {
        let d = dict();
        let words = Words {
            verbs: &[],
            words: &["さん", "高さ", "よく", "田中"],
        };
        assert_eq!(
            d.check_answer("sa-particle", "田中さんが来た。", &words),
            Verdict::NotFound
        );
        assert_eq!(
            d.check_answer("sa-particle", "それがさ、困ったんだ。", &words),
            Verdict::Accepted
        );
        // A word before it ends on it: the さ of 高さ passes.
        assert_eq!(
            d.check_answer("sa-particle", "高さが違う。", &words),
            Verdict::Accepted
        );
        assert_eq!(
            d.check_answer("yo", "よく食べる。", &words),
            Verdict::NotFound
        );
        assert_eq!(
            d.check_answer("yo", "いい天気だよ。", &words),
            Verdict::Accepted
        );
        // A kana word that is a point itself doesn't hide it.
        let with_yone = Words {
            verbs: &[],
            words: &["よね"],
        };
        assert_eq!(
            d.check_answer("yo", "いいよね。", &with_yone),
            Verdict::Accepted
        );
    }

    #[test]
    fn a_verdict_travels_as_json() {
        let v = Verdict::Close {
            related: related("ta-ato-de", "たあとで"),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"close","related":{"id":"ta-ato-de","pattern":"たあとで"}}"#
        );
        assert_eq!(serde_json::from_str::<Verdict>(&json).unwrap(), v);
        assert_eq!(
            serde_json::to_string(&Verdict::NotFound).unwrap(),
            r#"{"kind":"not-found"}"#
        );
    }
}
