//! Prints one converted entry, for looking at the converter's output:
//! `GRAMMAR_ZIP=… GRAMMAR_TERM=てよかった cargo test --release -p grammar-core --test dump_entry -- --ignored --nocapture`

#![allow(clippy::unwrap_used, clippy::print_stderr)]

use grammar_core::types::plain;
use grammar_core::{Block, GrammarDict, Span, convert};

fn show(rich: &[Span]) -> String {
    rich.iter()
        .map(|s| match s {
            Span::Highlight { text } => format!("[{text}]"),
            Span::LineBreak => "⏎".into(),
            other => other.text().to_owned(),
        })
        .collect()
}

#[test]
#[ignore = "needs GRAMMAR_ZIP and GRAMMAR_TERM"]
fn dump() {
    let zip = std::fs::read(std::env::var("GRAMMAR_ZIP").unwrap()).unwrap();
    let term = std::env::var("GRAMMAR_TERM").unwrap();
    let dict = GrammarDict::from_bytes(&convert(&zip).unwrap().bytes).unwrap();
    for e in dict.lookup(&term) {
        eprintln!(
            "== {} | meaning: {} | structure: {}",
            e.term, e.meaning, e.structure
        );
        eprintln!("   full meaning: {}", e.full_meaning());
        let main = e.main_source();
        eprintln!(
            "   main source: {}",
            e.sources.get(main).map_or("-", |s| s.name.as_str())
        );
        if std::env::var_os("GRAMMAR_PROSE").is_some() {
            for s in &e.sources {
                eprintln!("-- prose of {}", s.name);
                for sense in &s.senses {
                    for (kind, text) in sense.prose() {
                        eprintln!("     {kind:?}: {}", show(text));
                    }
                }
            }
            continue;
        }
        for s in &e.sources {
            eprintln!("-- source {} ({})", s.name, s.level);
            for sense in &s.senses {
                eprintln!("   sense '{}'", sense.label);
                for b in &sense.blocks {
                    match b {
                        Block::Paragraph { kind, text } => {
                            eprintln!("     {kind:?}: {}", show(text))
                        }
                        Block::Patterns { items } => {
                            eprintln!(
                                "     patterns: {:?}",
                                items.iter().map(|i| plain(i)).collect::<Vec<_>>()
                            )
                        }
                        Block::Examples { items } => {
                            for ex in items {
                                eprintln!("     ex: {} / {}", show(&ex.ja), ex.en);
                            }
                        }
                    }
                }
            }
        }
    }
}
