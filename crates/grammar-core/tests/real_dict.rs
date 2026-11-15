//! Conversion of a real Yomitan grammar dictionary, which can't live in this
//! repository. Download Bee's Ultimate Grammar Dictionary somewhere outside
//! it and run:
//!
//! ```sh
//! GRAMMAR_ZIP=path/to/bees-ultimate-grammar-dictionary.zip \
//!     cargo test --release -p grammar-core --test real_dict -- --ignored --nocapture
//! ```

#![allow(clippy::panic, clippy::print_stderr, clippy::unwrap_used)]

use std::time::Instant;

use grammar_core::{Block, GrammarDict, convert};

#[test]
#[ignore = "needs GRAMMAR_ZIP"]
fn converts_and_looks_up_the_real_dictionary() {
    let path = std::env::var("GRAMMAR_ZIP").expect("set GRAMMAR_ZIP");
    let zip = std::fs::read(&path).unwrap();

    let t = Instant::now();
    let converted = convert(&zip).unwrap();
    let convert_time = t.elapsed();

    // The longest step is how long a page stays frozen at once.
    let mut stepper = grammar_core::Converter::new(zip.clone()).unwrap();
    let mut steps = Vec::new();
    loop {
        let t = Instant::now();
        let more = stepper.step().unwrap();
        steps.push(t.elapsed());
        if !more {
            break;
        }
    }
    let t = Instant::now();
    let stepped = stepper.finish().unwrap();
    eprintln!("steps {steps:.2?}, finish {:.2?}", t.elapsed());
    assert_eq!(stepped.bytes, converted.bytes);
    let t = Instant::now();
    let dict = GrammarDict::from_bytes(&converted.bytes).unwrap();
    let load_time = t.elapsed();
    eprintln!(
        "{} rev {}: {} entries, {} keys, {:.1} MB from a {:.1} MB ZIP; convert {convert_time:.2?}, load {load_time:.2?}",
        converted.meta.title,
        converted.meta.revision,
        converted.entries,
        dict.key_count(),
        converted.bytes.len() as f64 / 1e6,
        zip.len() as f64 / 1e6,
    );
    assert!(converted.entries > 5000);

    // Every entry must keep something to show.
    let all = dict.lookup_prefix("", usize::MAX);
    assert_eq!(all.len(), converted.entries);
    let empty: Vec<&str> = all
        .iter()
        .filter(|e| e.meaning.is_empty() && e.sources.is_empty() && e.see_also.is_empty())
        .map(|e| e.term.as_str())
        .collect();
    eprintln!(
        "{} entries with nothing to show: {:?}",
        empty.len(),
        &empty[..empty.len().min(10)]
    );
    let senses_without_blocks = all
        .iter()
        .flat_map(|e| &e.sources)
        .flat_map(|s| &s.senses)
        .filter(|s| s.blocks.is_empty())
        .count();
    let examples: usize = all
        .iter()
        .flat_map(|e| &e.sources)
        .flat_map(|s| &s.senses)
        .flat_map(|s| &s.blocks)
        .map(|b| match b {
            Block::Examples { items } => items.len(),
            _ => 0,
        })
        .sum();
    // Picture-only senses (絵でわかる's illustrated posts): their content is
    // the image this format leaves to the website.
    eprintln!("{senses_without_blocks} senses without blocks, {examples} examples");
    assert!(
        empty.len() < converted.entries / 100,
        "too many empty entries"
    );

    let t = Instant::now();
    for (text, term) in [
        // Conjugated forms that aren't headwords themselves.
        ("ようにしなかった。", "ようにする"),
        ("ことにした。", "ことにする"),
        ("てしまった", "てしまう"),
        ("わけではなかった", "わけではない"),
        // Patterns that don't conjugate.
        ("にもかかわらず雨が", "にもかかわらず"),
        ("ばかりか", "ばかりか"),
    ] {
        let (entries, len) = dict
            .lookup_longest_match(text, 20)
            .unwrap_or_else(|| panic!("no match for {text}"));
        let terms: Vec<&str> = entries.iter().map(|e| e.term.as_str()).collect();
        eprintln!("{text} → {terms:?} ({len} chars)");
        assert!(terms.contains(&term), "{text}: {terms:?}");
    }
    eprintln!("lookups {:.2?}", t.elapsed());

    for (text, term) in [("てよかった", "てよかった"), ("来てよかった", "てよかった")]
    {
        let t = Instant::now();
        let terms: Vec<String> = dict.search(text, 10).into_iter().map(|e| e.term).collect();
        eprintln!("search {text} → {terms:?} in {:.2?}", t.elapsed());
        assert_eq!(terms.first().map(String::as_str), Some(term), "{text}");
    }
}
