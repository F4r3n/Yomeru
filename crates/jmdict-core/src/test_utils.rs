//! Builders for small in-memory dictionaries, for tests and benches here and
//! in crates that call the lookup (the server's endpoint tests). Behind the
//! `test-utils` feature so production builds don't carry them.
// Fixtures: a malformed one is a bug in the test, so failing loudly is right.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;

use fst::MapBuilder;
use jmdict_types::{Gloss, KanjiElement, PartOfSpeech, ReadingElement, Sense, WordEntry};
use postcard::to_allocvec;

/// An entry with one kanji form (none if `kanji` is empty), one reading and
/// one sense holding a single gloss.
pub fn word(
    sequence: u32,
    kanji: &str,
    reading: &str,
    pos: Vec<PartOfSpeech>,
    gloss: &str,
) -> WordEntry {
    WordEntry {
        sequence,
        kanji_forms: if kanji.is_empty() {
            vec![]
        } else {
            vec![KanjiElement::from_text(kanji)]
        },
        reading_forms: vec![ReadingElement::from_reading(reading)],
        senses: vec![Sense {
            pos,
            glosses: vec![Gloss {
                text: gloss.into(),
                gloss_type: None,
            }],
            ..Sense::default()
        }],
    }
}

/// The `jmdict.bin` bytes for `entries`, keyed by every kanji form and
/// reading the way `jmdict-build`'s indexer keys them. Load it with
/// [`crate::init`].
pub fn build_binary(entries: &[WordEntry]) -> Vec<u8> {
    let mut entries_bytes: Vec<u8> = Vec::new();
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(entries.len());
    let mut seq_pairs: Vec<(u32, u32)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let serialized = rkyv::to_bytes::<rkyv::rancor::Error>(entry).unwrap();
        let offset = entries_bytes.len() as u32;
        entry_offsets.push(offset);
        seq_pairs.push((entry.sequence, offset));
        entries_bytes.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
        entries_bytes.extend_from_slice(&serialized);
    }
    // The seq index is binary-searched at runtime.
    seq_pairs.sort_unstable_by_key(|(s, _)| *s);

    let mut key_to_indices: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (entry, &byte_offset) in entries.iter().zip(&entry_offsets) {
        let keys = entry
            .kanji_forms
            .iter()
            .map(|k| &k.text)
            .chain(entry.reading_forms.iter().map(|r| &r.text));
        for key in keys {
            key_to_indices
                .entry(key.clone())
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
        fst_map.insert(key.into_bytes(), u64::from(group_idx));
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
    // v5: rkyv-archived entries + trailing seq index section.
    out.push(5u8);
    for blob in [&fst_bytes, &lt_bytes, &entries_bytes, &seq_bytes] {
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(blob);
    }
    out
}
