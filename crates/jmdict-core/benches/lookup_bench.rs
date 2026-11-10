use criterion::{Criterion, black_box, criterion_group, criterion_main};
use jmdict_core::lookup::{lookup, lookup_longest_match, lookup_prefix};
use jmdict_core::lookup_by_sequence;
use jmdict_core::test_utils::word as make_entry;
use jmdict_types::PartOfSpeech;
use std::sync::Once;

static DICT_INIT: Once = Once::new();

fn setup() {
    DICT_INIT.call_once(|| {
        let bytes = build_test_binary();
        jmdict_core::init_for_testing(&bytes).expect("dict init failed");
    });
}

/// Sequence number of the deepest filler entry, used to bench a by-sequence
/// fetch that sits at the far end of the sorted seq index.
const DEEP_SEQ: u32 = 1000 + (FILLER_COUNT - 1);
const FILLER_COUNT: u32 = 5000;

fn build_test_binary() -> Vec<u8> {
    let mut entries = vec![
        make_entry(
            1,
            "飲む",
            "のむ",
            vec![PartOfSpeech::VerbGodanMu],
            "to drink",
        ),
        make_entry(
            2,
            "食べる",
            "たべる",
            vec![PartOfSpeech::VerbIchidan],
            "to eat",
        ),
        make_entry(
            3,
            "美しい",
            "うつくしい",
            vec![PartOfSpeech::Adjective],
            "beautiful",
        ),
    ];
    // Filler entries give the seq index realistic depth so the by-sequence
    // binary search isn't trivially shallow. Each carries a unique reading so
    // it occupies its own FST key without colliding with the named entries.
    for i in 0..FILLER_COUNT {
        let seq = 1000 + i;
        entries.push(make_entry(
            seq,
            "",
            &format!("かな{i}"),
            vec![PartOfSpeech::Noun],
            "filler",
        ));
    }
    jmdict_core::test_utils::build_binary(&entries)
}

fn bench_lookup_exact(c: &mut Criterion) {
    setup();
    let mut g = c.benchmark_group("lookup_exact");

    g.bench_function("hit_kanji", |b| b.iter(|| lookup(black_box("食べる"))));

    g.bench_function("hit_reading", |b| b.iter(|| lookup(black_box("たべる"))));

    g.bench_function("miss", |b| b.iter(|| lookup(black_box("走る"))));

    g.finish();
}

fn bench_lookup_at(c: &mut Criterion) {
    setup();
    let mut g = c.benchmark_group("lookup_at");

    g.bench_function("surface_match", |b| {
        b.iter(|| lookup_longest_match(black_box("食べる話"), black_box(20)))
    });

    g.bench_function("deinflected_ichidan", |b| {
        b.iter(|| lookup_longest_match(black_box("食べられなかった"), black_box(20)))
    });

    g.bench_function("deinflected_godan", |b| {
        b.iter(|| lookup_longest_match(black_box("飲んでいなかった"), black_box(20)))
    });

    g.bench_function("miss", |b| {
        b.iter(|| lookup_longest_match(black_box("走った"), black_box(20)))
    });

    g.finish();
}

fn bench_lookup_prefix(c: &mut Criterion) {
    setup();
    let mut g = c.benchmark_group("lookup_prefix");

    g.bench_function("short_prefix_hit", |b| {
        b.iter(|| lookup_prefix(black_box("飲"), black_box(10)))
    });

    g.bench_function("miss", |b| {
        b.iter(|| lookup_prefix(black_box("走"), black_box(10)))
    });

    g.finish();
}

fn bench_lookup_by_sequence(c: &mut Criterion) {
    setup();
    let mut g = c.benchmark_group("lookup_by_sequence");

    // Fetch a named entry near the front of the sorted seq index.
    g.bench_function("hit_shallow", |b| {
        b.iter(|| lookup_by_sequence(black_box(2)))
    });

    // Fetch the deepest filler entry — worst case for the binary search.
    g.bench_function("hit_deep", |b| {
        b.iter(|| lookup_by_sequence(black_box(DEEP_SEQ)))
    });

    // Sequence that isn't in the index.
    g.bench_function("miss", |b| {
        b.iter(|| lookup_by_sequence(black_box(u32::MAX)))
    });

    g.finish();
}

criterion_group!(
    benches,
    bench_lookup_exact,
    bench_lookup_at,
    bench_lookup_prefix,
    bench_lookup_by_sequence
);
criterion_main!(benches);
