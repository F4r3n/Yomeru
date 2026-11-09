//! The lookup as plainly as it can be written: every prefix, longest first,
//! and every deinflected form of it looked up on its own. The real one
//! skips most of that work (one FST walk per position, no deinflection of
//! prefixes nothing can match), so it is checked against this one.
//!
//! Shared by the unit tests (tiny dictionary) and `tests/real_dict.rs`
//! (the full one). It is given the exact lookup to use, as the sequences of
//! a key's entries, since the two name this crate differently.

// Test-only code, kept as plain as possible: indexing reads better here.
#![allow(clippy::indexing_slicing)]

use std::collections::HashSet;

/// `lookup_longest_match(text, max_chars)`, as (sequences, match length).
pub fn longest_match(
    text: &str,
    max_chars: usize,
    exact: &impl Fn(&str) -> Vec<u32>,
) -> Option<(Vec<u32>, usize)> {
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .take(max_chars)
        .collect();
    for (i, &end) in ends.iter().enumerate().rev() {
        for form in deinflect::deinflect(&text[..end]) {
            let seqs = exact(&form.text);
            if !seqs.is_empty() {
                return Some((seqs, i + 1));
            }
        }
    }
    None
}

/// `find_in_text(text, known)`: greedy segmentation, each match skipped as
/// a whole, offsets in UTF-16 units.
pub fn find_in_text(
    text: &str,
    known: &HashSet<u32>,
    exact: &impl Fn(&str) -> Vec<u32>,
) -> Vec<[usize; 2]> {
    if known.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let utf16 = |cs: &[char]| cs.iter().map(|c| c.len_utf16()).sum::<usize>();
    let mut out = Vec::new();
    let mut i = 0;
    // UTF-16 offset of chars[i].
    let mut at = 0;
    while i < chars.len() {
        if !japanese_utils::is_japanese(chars[i]) {
            at += chars[i].len_utf16();
            i += 1;
            continue;
        }
        let run = chars[i..]
            .iter()
            .take(20)
            .take_while(|c| japanese_utils::is_japanese(**c))
            .count();
        // A match is never longer than the run, so the rest past it can't
        // change the answer.
        let rest: String = chars[i..i + run].iter().collect();
        match longest_match(&rest, run, exact) {
            Some((seqs, len)) => {
                let width = utf16(&chars[i..i + len]);
                if seqs.iter().any(|s| known.contains(s)) {
                    out.push([at, width]);
                }
                at += width;
                i += len;
            }
            None => {
                at += chars[i].len_utf16();
                i += 1;
            }
        }
    }
    out
}
