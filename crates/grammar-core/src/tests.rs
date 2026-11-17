//! Hand-written entries in the shape of `data/grammar.toml`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use crate::types::{Span, parse_marked, plain};
use crate::{GrammarDict, compile};

/// One `[[point]]`, the fields the tests vary given, the rest filled in.
fn point(id: &str, pattern: &str, forms: &[&str], extra: &str) -> String {
    let forms: Vec<String> = forms.iter().map(|f| format!("{f:?}")).collect();
    format!(
        r#"
[[point]]
id = "{id}"
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
  {{ source = "yomeru", ja = "{{{pattern}}}だ。", en = "Ours." }},
]
{extra}
"#,
        forms = forms.join(", ")
    )
}

fn sample() -> String {
    [
        point(
            "you-ni-suru",
            "ようにする",
            &["ようにします"],
            r#"related = ["you-ni-naru", "not-written"]"#,
        ),
        point("you-ni-naru", "ようになる", &[], ""),
        point("you-ni", "ように", &[], ""),
        point("te-yokatta", "てよかった", &[], ""),
        point("yokatta", "よかった", &[], ""),
        point("tte", "って", &[], "hover = false"),
        point("wa-particle", "は", &[], ""),
        point("shidai", "次第", &[], r#"homographs = ["shidai-de"]"#),
        point(
            "shidai-de",
            "次第で",
            &["次第"],
            r#"homographs = ["shidai"]"#,
        ),
    ]
    .concat()
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
    assert_eq!(compiled.entries, 9);
    assert_eq!(
        compiled.warnings,
        ["you-ni-suru: related not-written isn't written yet"]
    );
    let dict = GrammarDict::from_bytes(&compiled.bytes).unwrap();
    assert_eq!(dict.len(), 9);
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
    assert!(!dict.by_id("tte").unwrap().hover);
    assert!(dict.by_id("missing").is_none());
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
    let dict = GrammarDict::from_bytes(&compile(&src).unwrap().bytes).unwrap();
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
    ];
    for (toml, want) in cases {
        let err = format!("{:#}", compile(&toml).err().unwrap());
        assert!(err.contains(want), "{want}: {err}");
    }
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
