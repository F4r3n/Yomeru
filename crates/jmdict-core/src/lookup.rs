use std::collections::HashSet;

use jmdict_types::ArchivedWordEntry;

use crate::dictionary::{fst_get, fst_prefix_search, get_entry, get_entry_group};

/// Exact lookup by headword or reading.
///
/// Entries are returned as zero-copy references into the global dictionary
/// buffer (`'static`, never mutated after init).
pub fn lookup(text: &str) -> Vec<&'static ArchivedWordEntry> {
    let Some(group_idx) = fst_get(text) else {
        return vec![];
    };
    let Some(indices) = get_entry_group(group_idx) else {
        return vec![];
    };
    indices.iter().filter_map(|&i| get_entry(i)).collect()
}

/// Hover lookup: deinflect `text`, then for each candidate form try
/// longest-match against the dictionary. Returns entries for the first
/// (longest / least-transformed) match found, plus the char count of the
/// surface form that produced the match (used for highlighting).
pub fn lookup_longest_match(
    text: &str,
    max_chars: usize,
) -> Option<(Vec<&'static ArchivedWordEntry>, usize)> {
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

    let mut seen = HashSet::new();

    for &end_byte in boundaries.iter().rev().filter(|&&b| b > 0) {
        let Some(&char_count) = byte_to_chars.get(&end_byte) else {
            continue;
        };
        let surface = &text[..end_byte];

        if seen.insert(surface.to_string())
            && let Some(entries) = try_get_entries(surface)
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

fn try_get_entries(candidate: &str) -> Option<Vec<&'static ArchivedWordEntry>> {
    if let Some(group_idx) = fst_get(candidate)
        && let Some(indices) = get_entry_group(group_idx)
    {
        let entries: Vec<&'static ArchivedWordEntry> =
            indices.iter().filter_map(|&i| get_entry(i)).collect();

        if !entries.is_empty() {
            return Some(entries);
        }
    }
    None
}

/// Prefix search: find entries whose headword *starts with* `text`.
/// Useful for autocomplete / options search UI.
pub fn lookup_prefix(text: &str, max_results: u8) -> Vec<&'static ArchivedWordEntry> {
    let max = max_results as usize;
    let mut seen = std::collections::BTreeSet::new();
    let mut entries: Vec<&'static ArchivedWordEntry> = Vec::new();

    // Exact match first, then FST prefix hits.
    let exact = fst_get(text)
        .and_then(get_entry_group)
        .unwrap_or_default();
    for i in exact {
        if seen.insert(i)
            && let Some(e) = get_entry(i)
        {
            entries.push(e);
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
                if seen.insert(i)
                    && let Some(e) = get_entry(i)
                {
                    entries.push(e);
                }
            }
        }
    }

    entries
}

/// Scan `text` for all positions matching words in `known` (a set of headwords).
/// Returns `[char_start, match_len_chars]` pairs. Non-Japanese chars are skipped;
/// matched segments are advanced past to avoid double-counting.
pub fn find_in_text(text: &str, known: &HashSet<String>) -> Vec<[usize; 2]> {
    if known.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<[usize; 2]> = Vec::new();
    let mut iter = text.char_indices().enumerate().peekable();

    while let Some(&(ci, (byte_off, ch))) = iter.peek() {
        if !japanese_utils::is_japanese(ch) {
            iter.next();
            continue;
        }
        match lookup_longest_match(&text[byte_off..], 20) {
            Some((entries, match_len)) => {
                let hw = entries
                    .first()
                    .and_then(|e| {
                        e.kanji_forms
                            .first()
                            .map(|k| k.text.as_str())
                            .or_else(|| e.reading_forms.first().map(|r| r.text.as_str()))
                    })
                    .unwrap_or("");
                if !hw.is_empty() && known.contains(hw) {
                    results.push([ci, match_len]);
                    for _ in 0..match_len {
                        if iter.next().is_none() {
                            break;
                        }
                    }
                } else {
                    iter.next();
                }
            }
            None => {
                iter.next();
            }
        }
    }

    results
}
