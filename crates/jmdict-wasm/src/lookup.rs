use jmdict_types::WordEntry;
use wasm_bindgen::prelude::*;

use crate::dictionary::{fst_get, fst_prefix_search, get_entry, get_entry_group};

/// Exact lookup by headword or reading.
pub fn lookup(text: &str) -> Result<Vec<WordEntry>, JsError> {
    let Some(group_idx) = fst_get(text) else {
        return Ok(vec![]);
    };
    let Some(indices) = get_entry_group(group_idx) else {
        return Ok(vec![]);
    };
    Ok(indices.iter().filter_map(|&i| get_entry(i)).collect())
}

/// Hover lookup: deinflect `text`, then for each candidate form try
/// longest-match against the dictionary. Returns entries for the first
/// (longest / least-transformed) match found, plus the char count of the
/// surface form that produced the match (used for highlighting).
pub fn lookup_longest_match(text: &str, max_chars: usize) -> Option<(Vec<WordEntry>, usize)> {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .take(max_chars + 1)
        .collect();

    // Map byte_end → number of chars in that prefix (boundaries[k] = k-char prefix end).
    let byte_to_chars: std::collections::HashMap<usize, usize> = boundaries
        .iter()
        .enumerate()
        .map(|(i, &b)| (b, i))
        .collect();

    let mut seen = std::collections::HashSet::new();

    for &end_byte in boundaries.iter().rev().filter(|&&b| b > 0) {
        let char_count = byte_to_chars[&end_byte];
        let surface = &text[..end_byte];

        if seen.insert(surface.to_string())
            && let Some(entries) = try_get_entries(&surface)
        {
            return Some((entries, char_count));
        }
        for d in deinflect::deinflect(surface) {
            if seen.insert(d.text.clone())
                && let Some(entries) = try_get_entries(&d.text)
            {
                return Some((entries, char_count));
            }
        }
    }

    None
}

fn try_get_entries(candidate: &str) -> Option<Vec<WordEntry>> {
    if let Some(group_idx) = fst_get(&candidate)
        && let Some(indices) = get_entry_group(group_idx)
    {
        let entries: Vec<WordEntry> = indices.iter().filter_map(|&i| get_entry(i)).collect();

        if !entries.is_empty() {
            return Some(entries);
        }
    }
    None
}

/// Prefix search: find entries whose headword *starts with* `text`.
/// Useful for autocomplete / options search UI.
pub fn lookup_prefix(text: &str, max_results: u8) -> Result<Vec<WordEntry>, JsError> {
    let max = max_results as usize;
    let mut seen = std::collections::BTreeSet::new();
    let mut entries = Vec::new();

    // Exact match first, then FST prefix hits.
    let exact = fst_get(text)
        .and_then(|g| get_entry_group(g))
        .unwrap_or_default();
    for i in exact {
        if seen.insert(i) {
            if let Some(e) = get_entry(i) {
                entries.push(e);
            }
        }
    }

    if entries.len() < max {
        for (_key, group_idx) in fst_prefix_search(text) {
            if entries.len() >= max {
                break;
            }
            for i in get_entry_group(group_idx).unwrap_or_default() {
                if entries.len() >= max {
                    break;
                }
                if seen.insert(i) {
                    if let Some(e) = get_entry(i) {
                        entries.push(e);
                    }
                }
            }
        }
    }

    Ok(entries)
}
