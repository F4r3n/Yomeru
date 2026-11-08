//! Benchmarks against the *real* JMdict binary: `lookup_by_sequence` (entry
//! sizes are representative — real entries have many senses/glosses/tags,
//! unlike the tiny synthetic entries in `lookup_bench`) and `find_in_text`
//! (the real FST has ~200k entries' worth of keys, so per-position prefix
//! probing costs what it does in the extension). Skips gracefully if the
//! binary hasn't been built yet.
//!
//! Run: `cargo bench -p jmdict-core --bench real_dict_bench`

// Bench harness: unwrap/expect/indexing on known-good fixtures and stderr
// progress notes are fine here (clippy's *-in-tests exemptions don't cover benches).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stderr
)]

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::HashSet;
use std::sync::OnceLock;

const DEFAULT_BIN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../extension/data/jmdict.bin"
);

/// The dictionary binary must be built with the **same** `jmdict-types/full`
/// setting as this bench's reader (benches enable `full` via the dev-dep), or
/// postcard — which isn't self-describing — will fail to decode every entry and
/// the bench will find no sequences. Override the path with `JMDICT_BIN` to
/// point at a matching build.
fn bin_path() -> String {
    std::env::var("JMDICT_BIN").unwrap_or_else(|_| DEFAULT_BIN_PATH.to_string())
}

/// Loads the real dict once and returns a spread of sequences that actually
/// resolve to entries, or `None` if the binary is missing.
fn real_seqs() -> Option<&'static Vec<u32>> {
    static SEQS: OnceLock<Option<Vec<u32>>> = OnceLock::new();
    SEQS.get_or_init(|| {
        let path = bin_path();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skipping: cannot read {path}: {e}");
                return None;
            }
        };
        jmdict_core::init(&bytes).expect("real dict init failed");
        // Probe a wide ent_seq range and keep the hits — a realistic mix of
        // entry sizes, like the batch the Word List resolves.
        let seqs: Vec<u32> = (1_000_000..9_000_000)
            .step_by(7)
            .filter(|&s| jmdict_core::lookup_by_sequence(s).is_some())
            .take(2000)
            .collect();
        eprintln!("real_dict_bench: collected {} sequences", seqs.len());
        (!seqs.is_empty()).then_some(seqs)
    })
    .as_ref()
}

fn bench_real_by_sequence(c: &mut Criterion) {
    let Some(seqs) = real_seqs() else {
        eprintln!("skipping real_dict_bench: {} not found", bin_path());
        return;
    };

    let mut g = c.benchmark_group("real_by_sequence");

    // Per-entry decode cost, averaged over real entry sizes.
    g.bench_function("single", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let s = seqs[i % seqs.len()];
            i += 1;
            jmdict_core::lookup_by_sequence(black_box(s))
        })
    });

    // The Word List shape: resolve a whole batch of sequences at once.
    g.bench_function(format!("batch_{}", seqs.len()), |b| {
        b.iter(|| {
            seqs.iter()
                .map(|&s| jmdict_core::lookup_by_sequence(black_box(s)))
                .collect::<Vec<_>>()
        })
    });

    // End-to-end server/extension edge: resolve a batch *and* serialize it to
    // JSON. This is where zero-copy pays off — with rkyv we serialize straight
    // from the archived buffer, never materializing an owned WordEntry.
    g.bench_function(format!("batch_to_json_{}", seqs.len()), |b| {
        b.iter(|| {
            let resolved: Vec<_> = seqs
                .iter()
                .map(|&s| jmdict_core::lookup_by_sequence(black_box(s)))
                .collect();
            serde_json::to_string(&resolved).unwrap()
        })
    });

    g.finish();
}

/// A paragraph shaped like real page text: Japanese prose interleaved with
/// Latin words and punctuation. The highlighter runs `find_in_text` over every
/// text node on a page, so this per-position cost governs whether
/// re-highlighting after a DOM mutation stays imperceptible.
const PAGE_TEXT: &str = "\
今日はとても美しい日ですね。I had coffee at the cafe and 食べる something nice. \
友達と一緒にビールを飲むつもりです。The weather was lovely, so we walked around \
the park for a while. 明日also美しい天気になるといいですね。Let's meet again soon.";

/// Article-style prose with no Latin breaks: long Japanese runs mean every
/// position pays the full scan window, the worst case for the highlighter.
const ARTICLE_TEXT: &str = "\
週末に友達と一緒に駅の近くにある新しい喫茶店へ行きました。店の中は思ったより広くて、\
窓から公園の木がよく見えました。私は温かいコーヒーを飲みながら、先週買った本を少し読みました。\
友達は甘いケーキを食べて、最近始めた仕事の話をしてくれました。毎朝早く起きて電車で会社に通うのは\
大変だけれど、新しいことを勉強できるので楽しいそうです。帰りに公園を散歩していると、\
子供たちが元気に遊んでいました。夕方になると空が赤くなって、とても美しい景色でした。\
来週は雨が降るかもしれないと天気予報で言っていたので、次は映画を見に行く約束をしました。";

/// Words a learner plausibly knows that occur in the texts above (some in
/// conjugated form, so deinflection is exercised on hits).
const KNOWN_WORDS: &[&str] = &[
    "今日",
    "美しい",
    "友達",
    "一緒",
    "飲む",
    "食べる",
    "天気",
    "明日",
    "週末",
    "駅",
    "近く",
    "新しい",
    "喫茶店",
    "行く",
    "店",
    "広い",
    "窓",
    "公園",
    "木",
    "見る",
    "私",
    "温かい",
    "本",
    "読む",
    "甘い",
    "仕事",
    "話",
    "毎朝",
    "起きる",
    "電車",
    "会社",
    "大変",
    "勉強",
    "楽しい",
    "散歩",
    "子供",
    "遊ぶ",
    "空",
    "景色",
    "雨",
    "映画",
    "約束",
];

fn bench_real_find_in_text(c: &mut Criterion) {
    // `real_seqs` also loads the dictionary; reuse its sample as a
    // realistically sized known set (~2k cards) padded with the words that
    // actually appear in the texts.
    let Some(seqs) = real_seqs() else {
        eprintln!("skipping real find_in_text: {} not found", bin_path());
        return;
    };
    let mut known: HashSet<u32> = seqs.iter().copied().collect();
    let before = known.len();
    for w in KNOWN_WORDS {
        known.extend(
            jmdict_core::lookup(w)
                .iter()
                .map(|e| e.sequence.to_native()),
        );
    }
    eprintln!(
        "real find_in_text: {} known seqs ({} from text words)",
        known.len(),
        known.len() - before
    );
    // `find_in_text` short-circuits on an empty set, so "nothing known" uses a
    // sentinel that never matches.
    let none: HashSet<u32> = [u32::MAX].into_iter().collect();

    let mut g = c.benchmark_group("real_find_in_text");
    for (name, text, set) in [
        ("mixed_page_text", PAGE_TEXT, &known),
        ("dense_article", ARTICLE_TEXT, &known),
        ("dense_article_no_hits", ARTICLE_TEXT, &none),
    ] {
        // Per-char throughput makes page-size extrapolation trivial.
        g.throughput(Throughput::Elements(text.chars().count() as u64));
        g.bench_function(name, |b| {
            b.iter(|| jmdict_core::find_in_text(black_box(text), black_box(set)))
        });
    }
    g.finish();
}

criterion_group!(benches, bench_real_by_sequence, bench_real_find_in_text);
criterion_main!(benches);
