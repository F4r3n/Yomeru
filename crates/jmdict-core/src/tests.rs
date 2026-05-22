use std::collections::BTreeMap;

use fst::MapBuilder;
use jmdict_types::{Gloss, KanjiElement, PartOfSpeech, ReadingElement, Sense, WordEntry};
use once_cell::sync::OnceCell;
use postcard::to_allocvec;

use crate::dictionary::init_for_testing;
use crate::lookup::{lookup, lookup_longest_match, lookup_prefix};

static INIT: OnceCell<()> = OnceCell::new();

fn ensure_test_dict() {
    INIT.get_or_init(|| {
        let bytes = build_test_binary();
        init_for_testing(&bytes).expect("failed to init test dict");
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
                text: kanji.to_string(),
                info: vec![],
                priorities: vec![],
            }]
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
            glosses: vec![Gloss {
                text: gloss.to_string(),
                lang: "eng".to_string(),
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

fn build_test_binary() -> Vec<u8> {
    let entries = vec![
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

    let mut entries_bytes: Vec<u8> = Vec::new();
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(entries.len());
    for entry in &entries {
        let serialized = to_allocvec(entry).unwrap();
        entry_offsets.push(entries_bytes.len() as u32);
        entries_bytes.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
        entries_bytes.extend_from_slice(&serialized);
    }

    // Build key → group mapping (same logic as jmdict-build indexer)
    let mut key_to_indices: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        let byte_offset = entry_offsets[idx];
        for k in &entry.kanji_forms {
            key_to_indices
                .entry(k.text.clone())
                .or_default()
                .push(byte_offset);
        }
        for r in &entry.reading_forms {
            key_to_indices
                .entry(r.text.clone())
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

// ── lookup (exact) ────────────────────────────────────────────────────────────

#[test]
fn exact_lookup_by_kanji() {
    ensure_test_dict();
    let entries = lookup("飲む");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "飲む");
}

#[test]
fn exact_lookup_by_reading() {
    ensure_test_dict();
    let entries = lookup("のむ");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].primary_reading(), "のむ");
}

#[test]
fn exact_lookup_miss() {
    ensure_test_dict();
    let entries = lookup("走る");
    assert!(entries.is_empty());
}

// ── lookup_longest_match (surface + deinflection) ─────────────────────────────

#[test]
fn longest_match_surface_with_trailing_text() {
    ensure_test_dict();
    // "食べる話" — longest match should return 食べる
    let (entries, _match_len) = lookup_longest_match("食べる話", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "食べる");
}

#[test]
fn deinflect_ichidan_past() {
    ensure_test_dict();
    // 食べた → 食べる (ichidan: た → る)
    let (entries, _) = lookup_longest_match("食べた", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "食べる");
}

#[test]
fn deinflect_ichidan_negative() {
    ensure_test_dict();
    // 食べない → 食べる (ichidan: ない → る)
    let (entries, _match_len) = lookup_longest_match("食べない", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "食べる");
}

#[test]
fn deinflect_godan_te_form() {
    ensure_test_dict();
    // 飲んで → 飲む (godan mu: んで → む)
    let (entries, _match_len) = lookup_longest_match("飲んで", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "飲む");
}

#[test]
fn deinflect_godan_negative() {
    ensure_test_dict();
    // 飲まない → 飲む (godan mu: まない → む)
    let (entries, _match_len) = lookup_longest_match("飲まない", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "飲む");
}

#[test]
fn deinflect_godan_past() {
    ensure_test_dict();
    // 飲んだ → 飲む (godan mu: んだ → む)
    let (entries, _match_len) = lookup_longest_match("飲んだ", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "飲む");
}

#[test]
fn deinflect_i_adj_past() {
    ensure_test_dict();
    // 美しかった → 美しい (i-adj: かった → い)
    let (entries, _match_len) = lookup_longest_match("美しかった", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "美しい");
}

#[test]
fn deinflect_i_adj_negative() {
    ensure_test_dict();
    // 美しくない → 美しい (i-adj: くない → い)
    let (entries, _match_len) = lookup_longest_match("美しくない", 20).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].headword(), "美しい");
}

#[test]
fn longest_match_no_result() {
    ensure_test_dict();
    let result = lookup_longest_match("走った", 20);
    assert!(result.is_none());
}

#[test]
fn match_len_surface_exact() {
    ensure_test_dict();
    // Surface match: "食べる" = 3 chars
    let (entries, match_len) = lookup_longest_match("食べる", 20).unwrap();
    assert!(!entries.is_empty());
    assert_eq!(match_len, 3);
}

#[test]
fn match_len_surface_with_trailing() {
    ensure_test_dict();
    // "食べる話" — match is "食べる" (3 chars), "話" is trailing
    let (entries, match_len) = lookup_longest_match("食べる話", 20).unwrap();
    assert!(!entries.is_empty());
    assert_eq!(match_len, 3);
}

#[test]
fn match_len_deinflected() {
    ensure_test_dict();
    // "食べた" (3-char surface) deinflects to 食べる — surface was 3 chars
    let (_, match_len) = lookup_longest_match("食べた", 20).unwrap();
    assert_eq!(match_len, 3);
}

// ── lookup_prefix ─────────────────────────────────────────────────────────────

#[test]
fn prefix_search_finds_entry() {
    ensure_test_dict();
    let entries = lookup_prefix("飲", 10);
    assert!(entries.iter().any(|e| e.headword() == "飲む"));
}

#[test]
fn prefix_search_exact_match_included() {
    ensure_test_dict();
    // Exact key "食べる" also shows up in a prefix search for itself
    let entries = lookup_prefix("食べる", 10);
    assert!(entries.iter().any(|e| e.headword() == "食べる"));
}

#[test]
fn prefix_search_empty_on_no_match() {
    ensure_test_dict();
    let entries = lookup_prefix("走", 10);
    assert!(entries.is_empty());
}

#[test]
fn prefix_search_respects_max_results() {
    ensure_test_dict();
    // Asking for max 1 result should never return more
    let entries = lookup_prefix("", 1);
    assert!(entries.len() <= 1);
}
