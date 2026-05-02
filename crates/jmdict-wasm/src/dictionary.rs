use fst::Map;
use jmdict_types::WordEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct LookupAtResult {
    entries: Vec<WordEntry>,
    match_len: usize,
}

// The loaded dictionary instance, initialized once.
static DICT: OnceCell<DictionaryInner> = OnceCell::new();

struct DictionaryInner {
    fst: Map<Vec<u8>>,
    lookup_table: Vec<Vec<u32>>,
    entries_bytes: Vec<u8>,
}

/// The main dictionary object exposed to JavaScript.
/// Usage: `const dict = new Dictionary(bytes); dict.lookup("飲む");`
#[wasm_bindgen]
pub struct Dictionary {}

#[wasm_bindgen]
impl Dictionary {
    /// Load the binary dictionary produced by jmdict-build.
    #[wasm_bindgen(constructor)]
    pub fn new(dict_bytes: &[u8]) -> Result<Dictionary, JsError> {
        if DICT.get().is_some() {
            // Already loaded — reuse.
            return Ok(Dictionary {});
        }

        let inner = parse_binary(dict_bytes)
            .map_err(|e| JsError::new(&format!("Failed to load dictionary: {e}")))?;

        DICT.set(inner)
            .map_err(|_| JsError::new("Dictionary already initialized (race condition)"))?;

        Ok(Dictionary {})
    }

    /// Exact lookup by headword or reading.
    pub fn lookup(&self, text: &str) -> Result<JsValue, JsError> {
        let entries = crate::lookup::lookup(text)?;
        Ok(serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))?)
    }

    /// Hover lookup: tries the full text, then progressively shorter prefixes,
    /// returning the longest dictionary match found (longest-match algorithm).
    /// Returns `{ entries: WordEntry[], match_len: number }` where `match_len`
    /// is the number of Unicode chars in the matched surface text (for highlighting).
    pub fn lookup_at(&self, text: &str) -> Result<JsValue, JsError> {
        if let Some((entries, match_len)) = crate::lookup::lookup_longest_match(text, 20) {
            let result = LookupAtResult { entries, match_len };
            Ok(serde_wasm_bindgen::to_value(&result).map_err(|e| JsError::new(&e.to_string()))?)
        } else {
            Ok(JsValue::null())
        }
    }

    /// Prefix search: find entries whose headword starts with `text`.
    /// Useful for search UI autocomplete.
    pub fn lookup_prefix(&self, text: &str, max_results: u8) -> Result<JsValue, JsError> {
        let entries = crate::lookup::lookup_prefix(text, max_results)?;
        Ok(serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))?)
    }

    /// Returns true if the dictionary is loaded.
    pub fn is_loaded(&self) -> bool {
        DICT.get().is_some()
    }
}

pub(crate) fn fst_get(key: &str) -> Option<u64> {
    DICT.get()?.fst.get(key.as_bytes())
}

pub(crate) fn get_entry_group(group_idx: u64) -> Option<Vec<u32>> {
    DICT.get()
        .and_then(|d| d.lookup_table.get(group_idx as usize).cloned())
}

pub(crate) fn get_entry(idx: u32) -> Option<WordEntry> {
    let dict = DICT.get()?;
    let bytes = &dict.entries_bytes;
    let pos = idx as usize;
    if pos + 4 > bytes.len() {
        return None;
    }
    let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
    let start = pos + 4;
    if start + len > bytes.len() {
        return None;
    }
    from_bytes(&bytes[start..start + len]).ok()
}

pub(crate) fn fst_prefix_search(prefix: &str) -> Vec<(String, u64)> {
    let dict = match DICT.get() {
        Some(d) => d,
        None => return vec![],
    };
    use fst::Automaton;
    use fst::automaton::Str;
    let automaton = Str::new(prefix).starts_with();
    use fst::IntoStreamer;
    use fst::Streamer;
    let mut stream = dict.fst.search(automaton).into_stream();
    let mut results = Vec::new();
    while let Some((k, v)) = stream.next() {
        if let Ok(s) = std::str::from_utf8(k) {
            results.push((s.to_owned(), v));
        }
    }
    results
}

#[cfg(any(test, feature = "test-utils"))]
pub fn init_for_testing(bytes: &[u8]) -> anyhow::Result<()> {
    if DICT.get().is_some() {
        return Ok(());
    }
    let inner = parse_binary(bytes)?;
    DICT.set(inner)
        .map_err(|_| anyhow::anyhow!("DICT already set"))?;
    Ok(())
}

fn parse_binary(bytes: &[u8]) -> anyhow::Result<DictionaryInner> {
    use anyhow::bail;

    if bytes.len() < 9 {
        bail!("Dictionary binary too short");
    }
    if &bytes[0..4] != b"JMDI" {
        bail!("Invalid magic bytes");
    }
    if bytes[4] != 1 {
        bail!("Unsupported dictionary version {}", bytes[4]);
    }

    let mut pos = 5usize;

    let fst_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize;
    pos += 4;
    let fst_bytes = bytes[pos..pos + fst_len].to_vec();
    pos += fst_len;

    let lt_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize;
    pos += 4;
    let lookup_table: Vec<Vec<u32>> = from_bytes(&bytes[pos..pos + lt_len])?;
    pos += lt_len;

    let entries_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize;
    pos += 4;
    let entries_bytes = bytes[pos..pos + entries_len].to_vec();

    let fst = Map::new(fst_bytes)?;

    Ok(DictionaryInner {
        fst,
        lookup_table,
        entries_bytes,
    })
}
