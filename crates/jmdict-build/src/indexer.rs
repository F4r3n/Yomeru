use anyhow::{Result, bail};
use fst::MapBuilder;
use jmdict_types::WordEntry;
use postcard::to_allocvec;
use std::collections::BTreeMap;

pub struct DictionaryIndex {
    pub fst_bytes: Vec<u8>,
    pub entries_bytes: Vec<u8>,
    pub lookup_table_bytes: Vec<u8>,
    /// Sequence index: postcard-encoded `Vec<(u32, u32)>` of (ent_seq, byte_offset)
    /// pairs, sorted by ent_seq for binary search at runtime.
    pub seq_index_bytes: Vec<u8>,
}

pub fn build_index(entries: &[WordEntry]) -> Result<DictionaryIndex> {
    // Step 1: Serialize all entries and record their byte positions.
    let mut entries_bytes: Vec<u8> = Vec::new();
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(entries.len());
    let mut seq_pairs: Vec<(u32, u32)> = Vec::with_capacity(entries.len());

    for entry in entries {
        // rkyv archived layout: read back zero-copy at runtime. The `unaligned`
        // repr (see jmdict-types/Cargo.toml) lets the entry sit at any byte
        // offset in the blob, so the existing u32 length-prefix framing holds.
        let serialized = rkyv::to_bytes::<rkyv::rancor::Error>(entry)?;
        let offset = entries_bytes.len();
        if offset > u32::MAX as usize {
            bail!("jmdict entries blob exceeds 4 GiB ({} bytes)", offset);
        }
        entry_offsets.push(offset as u32);
        seq_pairs.push((entry.sequence, offset as u32));
        let len = serialized.len() as u32;
        entries_bytes.extend_from_slice(&len.to_le_bytes());
        entries_bytes.extend_from_slice(&serialized);
    }
    seq_pairs.sort_unstable_by_key(|(seq, _)| *seq);

    // Step 2: Build a BTreeMap of headword/reading → sorted list of entry indices.
    // BTreeMap because FST requires keys in sorted order.
    let mut key_to_indices: BTreeMap<String, Vec<u32>> = BTreeMap::new();

    for (entry, &byte_offset) in entries.iter().zip(entry_offsets.iter()) {
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

    // Step 3: Build lookup table (dedup groups of entry indices).
    let mut lookup_table: Vec<Vec<u32>> = Vec::new();
    let mut group_dedup: BTreeMap<Vec<u32>, u32> = BTreeMap::new();

    let mut fst_map: BTreeMap<Vec<u8>, u64> = BTreeMap::new();

    for (key, mut indices) in key_to_indices {
        indices.sort_unstable();
        indices.dedup();

        let group_idx = if let Some(&existing) = group_dedup.get(&indices) {
            existing
        } else {
            let g = lookup_table.len() as u32;
            group_dedup.insert(indices.clone(), g);
            lookup_table.push(indices);
            g
        };

        fst_map.insert(key.into_bytes(), u64::from(group_idx));
    }

    // Step 4: Build FST from sorted keys.
    let mut fst_builder = MapBuilder::memory();
    for (key, value) in &fst_map {
        fst_builder.insert(key, *value)?;
    }
    let fst_bytes = fst_builder.into_inner()?;

    // Step 5: Serialize lookup table and sequence index.
    let lookup_table_bytes = to_allocvec(&lookup_table)?;
    let seq_index_bytes = to_allocvec(&seq_pairs)?;

    Ok(DictionaryIndex {
        fst_bytes,
        entries_bytes,
        lookup_table_bytes,
        seq_index_bytes,
    })
}
