use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fst::MapBuilder;
use jmdict_core::lookup::{find_in_text, lookup, lookup_longest_match, lookup_prefix};
use jmdict_core::lookup_by_sequence;
use jmdict_types::{Gloss, KanjiElement, PartOfSpeech, ReadingElement, Sense, WordEntry};
use postcard::to_allocvec;
use std::collections::{BTreeMap, HashSet};
use std::sync::Once;

static DICT_INIT: Once = Once::new();

fn setup() {
    DICT_INIT.call_once(|| {
        let bytes = build_test_binary();
        jmdict_core::init_for_testing(&bytes).expect("dict init failed");
    });
}

fn make_entry(
    seq: u32,
    kanji: &str,
    reading: &str,
    pos: Vec<PartOfSpeech>,
    gloss: &str,
) -> WordEntry {
    WordEntry {
        sequence: seq,
        kanji_forms: if kanji.is_empty() {
            vec![]
        } else {
            vec![KanjiElement {
                text: kanji.into(),
                info: vec![],
                priorities: vec![],
            }]
        },
        reading_forms: vec![ReadingElement {
            text: reading.into(),
            no_kanji: false,
            restricted_to: vec![],
            info: vec![],
            priorities: vec![],
        }],
        senses: vec![Sense {
            pos,
            glosses: vec![Gloss {
                text: gloss.into(),
                gloss_type: None,
            }],
            xrefs: vec![],
            antonyms: vec![],
            fields: vec![],
            misc: vec![],
            info: vec![],
            dialects: vec![],
        }],
    }
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

    let mut entries_bytes: Vec<u8> = Vec::new();
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(entries.len());
    let mut seq_pairs: Vec<(u32, u32)> = Vec::with_capacity(entries.len());
    for entry in &entries {
        let serialized = rkyv::to_bytes::<rkyv::rancor::Error>(entry).unwrap();
        let offset = entries_bytes.len() as u32;
        entry_offsets.push(offset);
        seq_pairs.push((entry.sequence, offset));
        entries_bytes.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
        entries_bytes.extend_from_slice(&serialized);
    }
    // Seq index is sorted by ent_seq for binary search at runtime.
    seq_pairs.sort_unstable_by_key(|(seq, _)| *seq);

    let mut key_to_indices: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        let byte_offset = entry_offsets[idx];
        for k in &entry.kanji_forms {
            key_to_indices
                .entry(k.text.to_string())
                .or_default()
                .push(byte_offset);
        }
        for r in &entry.reading_forms {
            key_to_indices
                .entry(r.text.to_string())
                .or_default()
                .push(byte_offset);
        }
    }

    let mut lookup_table: Vec<Vec<u32>> = Vec::new();
    let mut group_dedup: BTreeMap<Vec<u32>, u32> = BTreeMap::new();
    let mut fst_map: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    for (key, mut indices) in key_to_indices {
        indices.sort_unstable();
        indices.dedup();
        let group_idx = if let Some(&g) = group_dedup.get(&indices) {
            g
        } else {
            let g = lookup_table.len() as u32;
            group_dedup.insert(indices.clone(), g);
            lookup_table.push(indices);
            g
        };
        fst_map.insert(key.into_bytes(), group_idx as u64);
    }

    let mut builder = MapBuilder::memory();
    for (k, v) in &fst_map {
        builder.insert(k, *v).unwrap();
    }
    let fst_bytes = builder.into_inner().unwrap();
    let lt_bytes = to_allocvec(&lookup_table).unwrap();
    let seq_bytes = to_allocvec(&seq_pairs).unwrap();

    let mut out = Vec::new();
    out.extend_from_slice(b"JMDI");
    out.push(5u8); // v5: rkyv-archived entries (Freq/enum tags) + trailing seq index section
    out.extend_from_slice(&(fst_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&fst_bytes);
    out.extend_from_slice(&(lt_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&lt_bytes);
    out.extend_from_slice(&(entries_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&entries_bytes);
    out.extend_from_slice(&(seq_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&seq_bytes);
    out
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

/// A paragraph shaped like real page text: Japanese prose interleaved with
/// Latin words and punctuation. The highlighter runs `find_in_text` over every
/// text node on a page, so the per-position cost here is what governs whether
/// re-highlighting after a DOM mutation stays imperceptible.
const PAGE_TEXT: &str = "\
今日はとても美しい日ですね。I had coffee at the cafe and 食べる something nice. \
友達と一緒にビールを飲むつもりです。The weather was lovely, so we walked around \
the park for a while. 明日also美しい天気になるといいですね。Let's meet again soon.";

fn bench_find_in_text(c: &mut Criterion) {
    setup();
    let mut g = c.benchmark_group("find_in_text");

    let known: HashSet<String> = ["飲む", "食べる", "美しい"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

    g.bench_function("mixed_page_text", |b| {
        b.iter(|| find_in_text(black_box(PAGE_TEXT), black_box(&known)))
    });

    // Pure Japanese: no run boundaries to cut the scan window short, so this is
    // the worst case for per-position work.
    g.bench_function("dense_japanese", |b| {
        b.iter(|| {
            find_in_text(
                black_box("友達と一緒にビールを飲むつもりですが明日も美しい天気になるといいですね"),
                black_box(&known),
            )
        })
    });

    // Nothing to match: every position pays the full miss path.
    g.bench_function("no_known_words_hit", |b| {
        let empty: HashSet<String> = ["走る".to_string()].into_iter().collect();
        b.iter(|| find_in_text(black_box(PAGE_TEXT), black_box(&empty)))
    });

    g.finish();
}

criterion_group!(
    benches,
    bench_lookup_exact,
    bench_lookup_at,
    bench_lookup_prefix,
    bench_lookup_by_sequence,
    bench_find_in_text
);
criterion_main!(benches);
