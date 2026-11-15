//! Hand-written entries in the shape of Bee's Ultimate Grammar Dictionary.
//! None of the real dictionary's content belongs in this repository.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::io::Write;

use serde_json::{Value, json};

use crate::types::{Block, ParagraphKind, Span, plain};
use crate::{GrammarDict, convert};

fn index() -> Value {
    json!({
        "title": "Test Grammar",
        "revision": "2026.01.01",
        "format": 3,
        "attribution": "Written for these tests",
        "url": "https://example.org/grammar",
        "indexUrl": "https://example.org/index.json",
        "downloadUrl": "https://example.org/grammar.zip",
        "sequenced": true
    })
}

fn zip_of(files: &[(&str, Value)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, value) in files {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn row(term: &str, reading: &str, rules: &str, card: Value) -> Value {
    json!([term, reading, "", rules, 0, [{ "type": "structured-content", "content": card }], 1, ""])
}

fn span(role: &str, content: Value) -> Value {
    json!({ "tag": "span", "data": { role: "" }, "content": content })
}

fn div(role: &str, content: Value) -> Value {
    json!({ "tag": "div", "data": { role: "" }, "content": content })
}

fn compact(meaning: &str, structure: &str, jlpt: &str) -> Value {
    div(
        "compact",
        json!([
            span("meaning", json!(meaning)),
            div(
                "metarow",
                json!([
                    span("structure", json!(structure)),
                    span("jlpt", json!(jlpt))
                ])
            ),
        ]),
    )
}

fn website(id: &str) -> Value {
    div(
        "website",
        json!({ "tag": "a", "href": format!("https://example.org/grammar/{id}/"), "content": "Read all explanations ↗" }),
    )
}

fn example(before: &str, hl: &str, after: &str, en: &str) -> Value {
    json!({ "tag": "li", "data": { "example": "" }, "content": [
        span("ja", json!([before, span("hl", json!(hl)), after])),
        span("en", json!(en)),
    ]})
}

/// "Try to": two sources, one with translated prose, patterns and
/// highlighted examples, one with a sense label and inline examples.
fn you_ni_suru() -> Value {
    div(
        "grammarCard",
        json!([
            compact("Make an effort to", "Vる+ようにする", "N4"),
            { "tag": "details", "data": { "sourceBlock": "" }, "content": [
                { "tag": "summary", "content": span("sourceName", json!("Source A")) },
                { "tag": "div", "content": [
                    div("sourceLevel", json!(["JLPT ", span("jlpt", json!("N4"))])),
                    div("sense", json!([
                        { "tag": "div", "data": { "prose": "" }, "lang": "ja", "content": [
                            div("proseTranslation", json!("Expresses an effort.")),
                            { "tag": "div", "content": "毎朝走るようにする。" },
                        ]},
                        { "tag": "ul", "data": { "patterns": "" }, "content": [
                            { "tag": "li", "data": { "pattern": "" }, "content": "Verb［る］ + ように + する" },
                        ]},
                        { "tag": "ul", "data": { "examples": "" }, "content": [
                            example("毎日運動する", "ようにし", "ている。", "I try to exercise every day."),
                            example("忘れない", "ようにする", "。", ""),
                        ]},
                    ])),
                ]},
            ]},
            { "tag": "details", "data": { "sourceBlock": "" }, "content": [
                { "tag": "summary", "content": span("sourceName", json!("Source B")) },
                { "tag": "div", "content": [
                    div("senseLabel", json!("Habits")),
                    div("sense", json!([
                        div("illustration", json!({ "tag": "img", "path": "media/x.avif" })),
                        { "tag": "div", "data": { "prose": "" }, "content": [
                            { "tag": "div", "content": [span("proseLabel", json!("【習慣】")), "習慣にする"] },
                            div("exampleLabel", json!("［例］")),
                            div("inlineExample", json!("早く寝るようにする")),
                            { "tag": "div", "data": { "exampleDerivation": "", "inlineExample": "" }, "content": "→早く寝る習慣をつける" },
                            { "tag": "div", "content": ["読み", { "tag": "ruby", "content": ["方", { "tag": "rt", "content": "かた" }] }, { "tag": "br" }, "次の行"] },
                        ]},
                    ])),
                ]},
            ]},
            website("you-ni-suru"),
        ]),
    )
}

fn sample_zip() -> Vec<u8> {
    let bank1 = json!([
        row("ようにする", "", "vs", you_ni_suru()),
        row(
            "ように",
            "",
            "",
            div(
                "grammarCard",
                json!([compact("So that", "Vる+ように", "N4"), website("you-ni")])
            )
        ),
    ]);
    let bank2 = json!([
        row(
            "AながらB（誤用例）",
            "",
            "",
            div(
                "grammarCard",
                json!([
                    div("compact", json!([])),
                    div(
                        "crossref",
                        json!(["see ", { "tag": "a", "href": "?query=A%E3%81%AA%E3%81%8C%E3%82%89B&wildcards=off", "content": "AながらB" }])
                    ),
                    website("nagara-misuse"),
                ])
            )
        ),
        row(
            "に因って",
            "によって",
            "",
            div(
                "grammarCard",
                json!([compact("By means of", "Nに因って", "N3")])
            )
        ),
        row(
            "によって",
            "",
            "",
            div(
                "grammarCard",
                json!([compact("Depending on", "Nによって", "N3")])
            )
        ),
    ]);
    zip_of(&[
        ("index.json", index()),
        ("term_bank_2.json", bank2),
        ("term_bank_1.json", bank1),
    ])
}

fn sample_dict() -> GrammarDict {
    GrammarDict::from_bytes(&convert(&sample_zip()).unwrap().bytes).unwrap()
}

#[test]
fn converts_the_index_into_meta() {
    let converted = convert(&sample_zip()).unwrap();
    assert_eq!(converted.entries, 5);
    let meta = &converted.meta;
    assert_eq!(meta.title, "Test Grammar");
    assert_eq!(meta.revision, "2026.01.01");
    assert_eq!(meta.index_url, "https://example.org/index.json");
    assert_eq!(sample_dict().meta(), meta);
}

#[test]
fn converts_a_card_into_sources_senses_and_blocks() {
    let e = &sample_dict().lookup("ようにする")[0];
    assert_eq!((e.term.as_str(), e.rules.as_str()), ("ようにする", "vs"));
    assert_eq!(e.meaning, "Make an effort to");
    assert_eq!(e.structure, "Vる+ようにする");
    assert_eq!(e.jlpt, "N4");
    assert_eq!(e.website, "https://example.org/grammar/you-ni-suru/");
    assert_eq!(e.sources.len(), 2);

    let a = &e.sources[0];
    assert_eq!((a.name.as_str(), a.level.as_str()), ("Source A", "N4"));
    let blocks = &a.senses[0].blocks;
    let Block::Paragraph { kind, text } = &blocks[0] else {
        panic!("{blocks:?}")
    };
    assert_eq!(
        (*kind, plain(text).as_str()),
        (ParagraphKind::Translation, "Expresses an effort.")
    );
    let Block::Paragraph { kind, text } = &blocks[1] else {
        panic!("{blocks:?}")
    };
    assert_eq!(
        (*kind, plain(text).as_str()),
        (ParagraphKind::Plain, "毎朝走るようにする。")
    );
    let Block::Patterns { items } = &blocks[2] else {
        panic!("{blocks:?}")
    };
    assert_eq!(plain(&items[0]), "Verb［る］ + ように + する");
    let Block::Examples { items } = &blocks[3] else {
        panic!("{blocks:?}")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].ja,
        vec![
            Span::Text {
                text: "毎日運動する".into()
            },
            Span::Highlight {
                text: "ようにし".into()
            },
            Span::Text {
                text: "ている。".into()
            },
        ]
    );
    assert_eq!(items[0].en, "I try to exercise every day.");
    assert_eq!(items[1].en, "");
}

#[test]
fn keeps_labels_inline_examples_and_ruby_and_drops_illustrations() {
    let e = &sample_dict().lookup("ようにする")[0];
    let b = &e.sources[1];
    assert_eq!(b.level, "");
    assert_eq!(b.senses[0].label, "Habits");
    let kinds: Vec<(ParagraphKind, String)> = b.senses[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { kind, text } => (*kind, plain(text)),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            (ParagraphKind::Plain, "【習慣】習慣にする".to_owned()),
            (ParagraphKind::ExampleLabel, "［例］".to_owned()),
            (
                ParagraphKind::InlineExample,
                "早く寝るようにする".to_owned()
            ),
            (
                ParagraphKind::Derivation,
                "→早く寝る習慣をつける".to_owned()
            ),
            (ParagraphKind::Plain, "読み方\n次の行".to_owned()),
        ]
    );
    let Block::Paragraph { text, .. } = &b.senses[0].blocks[0] else {
        unreachable!()
    };
    assert_eq!(
        text[0],
        Span::Label {
            text: "【習慣】".into()
        }
    );
    let Block::Paragraph { text, .. } = &b.senses[0].blocks[4] else {
        unreachable!()
    };
    assert!(
        text.contains(&Span::Ruby {
            text: "方".into(),
            ruby: "かた".into()
        }),
        "{text:?}"
    );
}

#[test]
fn a_cross_reference_keeps_its_target() {
    let e = &sample_dict().lookup("AながらB（誤用例）")[0];
    assert_eq!(e.see_also, ["AながらB"]);
    assert!(e.sources.is_empty());
    assert_eq!(e.meaning, "");
}

#[test]
fn looks_up_by_term_and_by_reading() {
    let dict = sample_dict();
    let by_reading: Vec<_> = dict
        .lookup("によって")
        .into_iter()
        .map(|e| e.meaning)
        .collect();
    // Dictionary order: the kanji headword's entry comes first.
    assert_eq!(by_reading, ["By means of", "Depending on"]);
    assert_eq!(dict.lookup("に因って").len(), 1);
    assert!(dict.lookup("によ").is_empty());
}

#[test]
fn longest_match_prefers_the_longer_pattern() {
    let dict = sample_dict();
    let (entries, len) = dict.lookup_longest_match("ようにする。", 20).unwrap();
    assert_eq!((entries[0].term.as_str(), len), ("ようにする", 5));
    let (entries, len) = dict.lookup_longest_match("ように頑張る", 20).unwrap();
    assert_eq!((entries[0].term.as_str(), len), ("ように", 3));
}

#[test]
fn longest_match_deinflects_conjugated_patterns() {
    let dict = sample_dict();
    for (text, len) in [
        ("ようにしている。", 7),
        ("ようにした。", 5),
        ("ようにしない", 6),
    ] {
        let (entries, matched) = dict
            .lookup_longest_match(text, 20)
            .unwrap_or_else(|| panic!("no match for {text}"));
        assert_eq!(
            (entries[0].term.as_str(), matched),
            ("ようにする", len),
            "{text}"
        );
    }
}

#[test]
fn longest_match_respects_max_chars_and_misses_cleanly() {
    let dict = sample_dict();
    let (entries, len) = dict.lookup_longest_match("ようにする", 3).unwrap();
    assert_eq!((entries[0].term.as_str(), len), ("ように", 3));
    assert!(dict.lookup_longest_match("食べる", 20).is_none());
    assert!(dict.lookup_longest_match("", 20).is_none());
}

#[test]
fn prefix_search_lists_each_entry_once() {
    let dict = sample_dict();
    let terms: Vec<_> = dict
        .lookup_prefix("よう", 10)
        .into_iter()
        .map(|e| e.term)
        .collect();
    assert_eq!(terms, ["ように", "ようにする"]);
    assert_eq!(dict.lookup_prefix("によ", 10).len(), 2);
    assert_eq!(dict.lookup_prefix("", 1).len(), 1);
}

#[test]
fn refuses_what_isnt_a_format_3_dictionary() {
    assert!(convert(b"not a zip").is_err());
    let no_index = zip_of(&[("term_bank_1.json", json!([]))]);
    assert!(convert(&no_index).is_err());
    let mut old = index();
    old["format"] = json!(1);
    assert!(
        convert(&zip_of(&[
            ("index.json", old),
            ("term_bank_1.json", json!([]))
        ]))
        .is_err()
    );
    let no_banks = zip_of(&[("index.json", index())]);
    assert!(convert(&no_banks).is_err());
}

#[test]
fn refuses_bad_or_other_version_bytes() {
    let mut bytes = convert(&sample_zip()).unwrap().bytes;
    assert!(GrammarDict::from_bytes(b"JMDI\x05").is_err());
    assert!(GrammarDict::from_bytes(&bytes[..bytes.len() - 3]).is_err());
    bytes[4] = crate::FORMAT_VERSION + 1;
    let err = GrammarDict::from_bytes(&bytes).err().unwrap().to_string();
    assert!(err.contains("import it again"), "{err}");
}

#[test]
fn converts_in_steps_of_one_bank() {
    let mut c = crate::Converter::new(sample_zip()).unwrap();
    assert_eq!((c.done(), c.total()), (0, 2));
    assert_eq!(c.meta().title, "Test Grammar");
    assert!(c.step().unwrap());
    assert_eq!(c.done(), 1);
    assert!(!c.step().unwrap());
    assert!(!c.step().unwrap(), "stepping past the end is a no-op");
    assert_eq!(c.done(), 2);
    let stepped = c.finish().unwrap();
    assert_eq!(stepped.bytes, convert(&sample_zip()).unwrap().bytes);
}

#[test]
fn search_puts_exact_then_conjugated_then_inner_then_prefix_hits() {
    let dict = sample_dict();
    let terms =
        |text: &str| -> Vec<String> { dict.search(text, 10).into_iter().map(|e| e.term).collect() };
    assert_eq!(terms("ように"), ["ように", "ようにする"]);
    assert_eq!(terms("ようにしなかった"), ["ようにする"]);
    // Inside a sentence, by position; each entry once.
    assert_eq!(terms("早く寝るようにした"), ["ようにする"]);
    assert_eq!(terms("雨によって"), ["に因って", "によって"]);
    assert!(terms("食べる").is_empty());
    assert!(terms("  ").is_empty());
    assert_eq!(dict.search("よう", 1).len(), 1);
}

#[test]
fn glued_subheadings_are_split_into_headings_and_items() {
    let card = div(
        "grammarCard",
        json!([
            { "tag": "details", "data": { "sourceBlock": "" }, "content": [
                { "tag": "summary", "content": span("sourceName", json!("Handbook")) },
                div("sense", json!([
                    { "tag": "div", "data": { "prose": "" }, "content":
                        "Expresses relief. b. V-なければよかった In the forms V-なければよかった, regret. c. V-ようか〈invitation〉" },
                    { "tag": "ul", "data": { "patterns": "" }, "content": [
                        { "tag": "li", "data": { "pattern": "" }, "content": "2. V-ばよかったa. V-ばよかった" },
                        { "tag": "li", "data": { "pattern": "" }, "content": "etc. Data" },
                    ]},
                ])),
            ]},
        ]),
    );
    let zip = zip_of(&[
        ("index.json", index()),
        ("term_bank_1.json", json!([row("よかった", "", "", card)])),
    ]);
    let dict = GrammarDict::from_bytes(&convert(&zip).unwrap().bytes).unwrap();
    let blocks = &dict.lookup("よかった")[0].sources[0].senses[0].blocks;
    let paragraphs: Vec<(ParagraphKind, String)> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph { kind, text } => Some((*kind, plain(text))),
            _ => None,
        })
        .collect();
    assert_eq!(
        paragraphs,
        [
            (ParagraphKind::Plain, "Expresses relief.".to_owned()),
            (ParagraphKind::Heading, "b. V-なければよかった".to_owned()),
            (
                ParagraphKind::Plain,
                "In the forms V-なければよかった, regret.".to_owned()
            ),
            (ParagraphKind::Plain, "c. V-ようか〈invitation〉".to_owned()),
        ]
    );
    let Some(Block::Patterns { items }) = blocks.last() else {
        panic!("{blocks:?}")
    };
    let items: Vec<String> = items.iter().map(|i| plain(i)).collect();
    assert_eq!(items, ["2. V-ばよかった", "a. V-ばよかった", "etc. Data"]);
}

fn para(kind: ParagraphKind, text: &str) -> Block {
    Block::Paragraph {
        kind,
        text: vec![Span::Text { text: text.into() }],
    }
}

fn source(name: &str, blocks: Vec<Block>) -> crate::SourceBlock {
    crate::SourceBlock {
        name: name.into(),
        level: String::new(),
        senses: vec![crate::Sense {
            label: String::new(),
            blocks,
        }],
    }
}

#[test]
fn prose_drops_paragraphs_repeating_an_example() {
    let sense = &source(
        "Bunpro",
        vec![
            para(ParagraphKind::Translation, "Means 'I'm glad that'."),
            para(ParagraphKind::Plain, "やっぱり今日来てよかったね。"),
            para(ParagraphKind::Translation, "Actually, I'm glad we came."),
            para(ParagraphKind::Plain, "行ったの？ 行かないでよかった！"),
            Block::Examples {
                items: vec![
                    crate::Example {
                        ja: vec![
                            Span::Text {
                                text: "やっぱり今日来".into(),
                            },
                            Span::Highlight {
                                text: "てよかった".into(),
                            },
                            Span::Text {
                                text: "ね。".into(),
                            },
                        ],
                        en: "Actually, I'm glad we came.".into(),
                    },
                    crate::Example {
                        ja: vec![
                            Span::Text {
                                text: "行ったの？".into(),
                            },
                            Span::LineBreak,
                            Span::Text {
                                text: "行かないでよかった！".into(),
                            },
                        ],
                        en: String::new(),
                    },
                ],
            },
        ],
    )
    .senses[0];
    let prose: Vec<String> = sense.prose().map(|(_, t)| plain(t)).collect();
    assert_eq!(prose, ["Means 'I'm glad that'."]);
    assert_eq!(sense.examples().count(), 2);
}

#[test]
fn a_cut_meaning_is_completed_and_the_main_source_explains_most_in_english() {
    let entry = crate::GrammarEntry {
        meaning: "Expresses relief after…".into(),
        sources: vec![
            source(
                "Short",
                vec![para(
                    ParagraphKind::Plain,
                    "Expresses relief after something happened.",
                )],
            ),
            source(
                "Japanese",
                vec![para(
                    ParagraphKind::Plain,
                    "物事が終わった後に、その物事に対する評価を行う文法です。",
                )],
            ),
            source(
                "Long",
                vec![para(
                    ParagraphKind::Translation,
                    "A much longer English explanation of the pattern.",
                )],
            ),
        ],
        ..Default::default()
    };
    assert_eq!(
        entry.full_meaning(),
        "Expresses relief after something happened."
    );
    assert_eq!(entry.main_source(), 2);

    let whole = crate::GrammarEntry {
        meaning: "Complete.".into(),
        ..entry.clone()
    };
    assert_eq!(whole.full_meaning(), "Complete.");
    let unknown = crate::GrammarEntry {
        meaning: "Nowhere else…".into(),
        ..entry
    };
    assert_eq!(unknown.full_meaning(), "Nowhere else…");
    assert_eq!(crate::GrammarEntry::default().main_source(), 0);
}
