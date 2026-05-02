use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fst::MapBuilder;
use jmdict_types::{Gloss, KanjiElement, PartOfSpeech, ReadingElement, Sense, WordEntry};
use jmdict_wasm::lookup::{lookup, lookup_longest_match, lookup_prefix};
use postcard::to_allocvec;
use std::collections::BTreeMap;
use std::sync::Once;

static DICT_INIT: Once = Once::new();

fn setup() {
    DICT_INIT.call_once(|| {
        let bytes = build_test_binary();
        jmdict_wasm::init_for_testing(&bytes).expect("dict init failed");
    });
}

fn make_entry(seq: u32, kanji: &str, reading: &str, pos: Vec<PartOfSpeech>, gloss: &str) -> WordEntry {
    WordEntry {
        sequence: seq,
        kanji_forms: if kanji.is_empty() {
            vec![]
        } else {
            vec![KanjiElement { text: kanji.to_string(), info: vec![], priorities: vec![] }]
        },
        reading_forms: vec![ReadingElement {
            text: reading.to_string(),
            no_kanji: false,
            restricted_to: vec![],
            info: vec![],
            priorities: vec![],
        }],
        senses: vec![Sense {
            pos,
            glosses: vec![Gloss { text: gloss.to_string(), lang: "eng".to_string(), gloss_type: None }],
            xrefs: vec![],
            antonyms: vec![],
            fields: vec![],
            misc: vec![],
            info: vec![],
            dialects: vec![],
        }],
    }
}

fn build_test_binary() -> Vec<u8> {
    let entries = vec![
        make_entry(1, "飲む",   "のむ",      vec![PartOfSpeech::VerbGodanMu],  "to drink"),
        make_entry(2, "食べる", "たべる",    vec![PartOfSpeech::VerbIchidan],  "to eat"),
        make_entry(3, "美しい", "うつくしい", vec![PartOfSpeech::Adjective],    "beautiful"),
    ];

    let mut entries_bytes: Vec<u8> = Vec::new();
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(entries.len());
    for entry in &entries {
        let serialized = to_allocvec(entry).unwrap();
        entry_offsets.push(entries_bytes.len() as u32);
        entries_bytes.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
        entries_bytes.extend_from_slice(&serialized);
    }

    let mut key_to_indices: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        let byte_offset = entry_offsets[idx];
        for k in &entry.kanji_forms {
            key_to_indices.entry(k.text.clone()).or_default().push(byte_offset);
        }
        for r in &entry.reading_forms {
            key_to_indices.entry(r.text.clone()).or_default().push(byte_offset);
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

    let mut out = Vec::new();
    out.extend_from_slice(b"JMDI");
    out.push(1u8);
    out.extend_from_slice(&(fst_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&fst_bytes);
    out.extend_from_slice(&(lt_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&lt_bytes);
    out.extend_from_slice(&(entries_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&entries_bytes);
    out
}

fn bench_lookup_exact(c: &mut Criterion) {
    setup();
    let mut g = c.benchmark_group("lookup_exact");

    g.bench_function("hit_kanji", |b| {
        b.iter(|| lookup(black_box("食べる")))
    });

    g.bench_function("hit_reading", |b| {
        b.iter(|| lookup(black_box("たべる")))
    });

    g.bench_function("miss", |b| {
        b.iter(|| lookup(black_box("走る")))
    });

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

criterion_group!(benches, bench_lookup_exact, bench_lookup_at, bench_lookup_prefix);
criterion_main!(benches);
