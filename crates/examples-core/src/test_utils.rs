//! Builders for small in-memory examples binaries, for tests here and in
//! crates that call the lookup (the server's endpoint tests). Behind the
//! `test-utils` feature so production builds don't carry them.
// Fixtures: a malformed one is a bug in the test, so failing loudly is right.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use examples_types::ExampleEntry;
use postcard::to_allocvec;

/// Build a small EXPL binary in-memory. `index` is (headword → ordered list
/// of sentence indices into `sentences`), so the test can control which
/// example offsets each headword resolves to.
pub fn build_binary(sentences: &[ExampleEntry], index: &[(&str, Vec<usize>)]) -> Vec<u8> {
    let mut sentences_bytes = Vec::new();
    let mut offsets = Vec::with_capacity(sentences.len());
    for s in sentences {
        offsets.push(sentences_bytes.len() as u32);
        let ser = to_allocvec(s).unwrap();
        sentences_bytes.extend_from_slice(&(ser.len() as u32).to_le_bytes());
        sentences_bytes.extend_from_slice(&ser);
    }

    // Lookup uses `binary_search_by`, so the index must be sorted by key.
    let mut idx: Vec<(String, Vec<u32>)> = index
        .iter()
        .map(|(k, ids)| (k.to_string(), ids.iter().map(|&i| offsets[i]).collect()))
        .collect();
    idx.sort_by(|a, b| a.0.cmp(&b.0));

    let index_bytes = to_allocvec(&idx).unwrap();

    let mut out = Vec::new();
    out.extend_from_slice(b"EXPL");
    out.push(1u8);
    out.extend_from_slice(&(index_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&index_bytes);
    out.extend_from_slice(&(sentences_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&sentences_bytes);
    out
}

/// A sentence pair.
pub fn ex(j: &str, e: &str) -> ExampleEntry {
    ExampleEntry {
        japanese: j.into(),
        english: e.into(),
    }
}
