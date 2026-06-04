//! Thin `#[wasm_bindgen]` shim over `jmdict-core`. All real logic lives in
//! `jmdict-core`; this crate exists only to expose it to JavaScript.

use jmdict_types::WordEntry;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_jmdict_wasm() {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());
}

#[derive(Serialize)]
struct LookupAtResult {
    entries: Vec<WordEntry>,
    match_len: usize,
}

/// JS-facing dictionary handle. State is held in a process-global in
/// `jmdict-core`; this struct is a zero-sized handle so the JS side has
/// something to call methods on.
#[wasm_bindgen]
pub struct Dictionary {}

#[wasm_bindgen]
impl Dictionary {
    #[wasm_bindgen(constructor)]
    pub fn new(dict_bytes: &[u8]) -> Result<Dictionary, JsError> {
        jmdict_core::init(dict_bytes)
            .map_err(|e| JsError::new(&format!("Failed to load dictionary: {e}")))?;
        Ok(Dictionary {})
    }

    /// Exact lookup by headword or reading.
    pub fn lookup(&self, text: &str) -> Result<JsValue, JsError> {
        let entries = jmdict_core::lookup(text);
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Hover lookup: tries the full text, then progressively shorter prefixes,
    /// returning the longest dictionary match found. Returns `null` if no match.
    /// Otherwise returns `{ entries: WordEntry[], match_len: number }` where
    /// `match_len` is the number of Unicode chars in the matched surface text.
    pub fn lookup_at(&self, text: &str) -> Result<JsValue, JsError> {
        if let Some((entries, match_len)) = jmdict_core::lookup_longest_match(text, 20) {
            let result = LookupAtResult { entries, match_len };
            serde_wasm_bindgen::to_value(&result).map_err(|e| JsError::new(&e.to_string()))
        } else {
            Ok(JsValue::null())
        }
    }

    /// Scan `text` for all positions matching words in `known` (a JS Array of strings).
    /// Returns a JS array of `[charStart, matchLen]` pairs.
    pub fn find_in_text(&self, text: &str, known: js_sys::Array) -> JsValue {
        let known_set: std::collections::HashSet<String> = known
            .iter()
            .filter_map(|v| v.as_string())
            .collect();
        let results = jmdict_core::find_in_text(text, &known_set);
        serde_wasm_bindgen::to_value(&results)
            .unwrap_or_else(|_| JsValue::from(js_sys::Array::new()))
    }

    /// Resolve JMdict `ent_seq` values to their entries. Takes a JS array of
    /// numbers and returns `Vec<Option<WordEntry>>` aligned by index (`null`
    /// for a sequence no longer in the dictionary). Used by SRS cards, which
    /// key on `sequence` rather than a surface string.
    pub fn lookup_by_sequence(&self, sequences: js_sys::Array) -> Result<JsValue, JsError> {
        let results: Vec<Option<WordEntry>> = sequences
            .iter()
            .map(|v| {
                v.as_f64()
                    .and_then(|n| jmdict_core::lookup_by_sequence(n as u32))
            })
            .collect();
        serde_wasm_bindgen::to_value(&results).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Prefix search: find entries whose headword starts with `text`.
    pub fn lookup_prefix(&self, text: &str, max_results: u8) -> Result<JsValue, JsError> {
        let entries = jmdict_core::lookup_prefix(text, max_results);
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn is_loaded(&self) -> bool {
        jmdict_core::dictionary::is_loaded()
    }
}

/// Extract the longest Japanese run from `text` starting at `char_offset`.
#[wasm_bindgen]
pub fn extract_japanese_run(text: &str, char_offset: usize) -> String {
    japanese_utils::extract_japanese_run(text, char_offset)
}

/// Returns true if the character (as a JS string of length 1) is Japanese.
#[wasm_bindgen]
pub fn is_japanese_str(s: &str) -> bool {
    s.chars().next().map(japanese_utils::is_japanese).unwrap_or(false)
}
