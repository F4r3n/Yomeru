//! Builders for small in-memory KANJIDIC binaries, for tests here and in
//! crates that call the lookup (the server's endpoint tests). Behind the
//! `test-utils` feature so production builds don't carry them.
// Fixtures: a malformed one is a bug in the test, so failing loudly is right.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use kanjidic_types::{KanjiEntry, KanjiPart, KanjiRecord};
use postcard::to_allocvec;

/// An entry with no grade, frequency, JLPT level, parts or mnemonic.
pub fn entry(
    literal: char,
    strokes: u8,
    on: &[&str],
    kun: &[&str],
    meanings: &[&str],
) -> KanjiEntry {
    KanjiEntry {
        literal,
        stroke_count: strokes,
        grade: None,
        freq: None,
        jlpt: None,
        on_readings: on.iter().map(|s| s.to_string()).collect(),
        kun_readings: kun.iter().map(|s| s.to_string()).collect(),
        meanings: meanings.iter().map(|s| s.to_string()).collect(),
        parts: vec![],
        mnemonic: None,
    }
}

/// Serialize a small in-memory KDIC binary matching the format documented
/// in `parse_binary` (version 2, so the kanji finder builds its part index
/// from the entries' `parts`). The index is sorted by codepoint to match the real
/// builder's invariant (`get_entry` does a binary search).
pub fn build_binary(entries: &[KanjiEntry]) -> Vec<u8> {
    let encoded = entries
        .iter()
        .map(|e| (e.literal, to_allocvec(&KanjiRecord::from(e)).unwrap()))
        .collect();
    build_raw(2, encoded)
}

/// The binary for already-encoded `(literal, entry bytes)` pairs, so a
/// test can lay out entries the way an older version did.
pub fn build_raw(version: u8, mut entries: Vec<(char, Vec<u8>)>) -> Vec<u8> {
    entries.sort_by_key(|(c, _)| *c as u32);

    let mut out = Vec::new();
    out.extend_from_slice(b"KDIC");
    out.push(version);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());

    let mut data = Vec::new();
    let mut offsets = Vec::with_capacity(entries.len());
    for (_, ser) in &entries {
        offsets.push(data.len() as u32);
        data.extend_from_slice(&(ser.len() as u32).to_le_bytes());
        data.extend_from_slice(ser);
    }
    for ((c, _), off) in entries.iter().zip(offsets.iter()) {
        out.extend_from_slice(&(*c as u32).to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// The parts of a kanji for [`KanjiEntry::parts`], plain elements only.
pub fn parts(elements: &[char]) -> Vec<KanjiPart> {
    elements
        .iter()
        .map(|&element| KanjiPart {
            element,
            original: None,
            radical: false,
            phonetic: false,
        })
        .collect()
}
