//! Thin `#[wasm_bindgen]` shim over `kanjidic-core`.

use kanjidic_types::KanjiEntry;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_kanjidic_wasm() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct KanjiDictionary {}

#[wasm_bindgen]
impl KanjiDictionary {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<KanjiDictionary, JsError> {
        kanjidic_core::init_from_bytes(bytes)
            .map_err(|e| JsError::new(&format!("Failed to load kanjidic: {e}")))?;
        Ok(KanjiDictionary {})
    }

    /// Lay the mnemonic file (`kanji-mnemonics.tsv`) over lookups.
    #[wasm_bindgen(js_name = withMnemonics)]
    pub fn with_mnemonics(&self, tsv: &str) -> Result<(), JsError> {
        kanjidic_core::init_mnemonics(tsv)
            .map_err(|e| JsError::new(&format!("Failed to load mnemonics: {e}")))
    }

    /// Lay the part-name file (`part-names.tsv`) over lookups.
    #[wasm_bindgen(js_name = withPartNames)]
    pub fn with_part_names(&self, tsv: &str) -> Result<(), JsError> {
        kanjidic_core::init_part_names(tsv)
            .map_err(|e| JsError::new(&format!("Failed to load part names: {e}")))
    }

    /// Single-char string → KanjiEntry | null
    pub fn lookup(&self, ch: &str) -> Result<JsValue, JsError> {
        let c = ch
            .chars()
            .next()
            .ok_or_else(|| JsError::new("empty string"))?;
        match kanjidic_core::lookup_one(c) {
            Some(e) => serde_wasm_bindgen::to_value(&e).map_err(|e| JsError::new(&e.to_string())),
            None => Ok(JsValue::null()),
        }
    }

    /// Extract kanji chars from word, return array of KanjiEntry for each found.
    pub fn lookup_many(&self, word: &str) -> Result<JsValue, JsError> {
        let entries: Vec<KanjiEntry> = kanjidic_core::lookup_many(word);
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Every part the kanji finder offers → PartInfo[]
    pub fn parts(&self) -> Result<JsValue, JsError> {
        serde_wasm_bindgen::to_value(&kanjidic_core::parts())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// The kanji holding every char of `parts` → PartSearch
    #[wasm_bindgen(js_name = withParts)]
    pub fn with_parts(&self, parts: &str, max: u8) -> Result<JsValue, JsError> {
        let parts: Vec<char> = parts.chars().collect();
        let found = kanjidic_core::kanji_with_parts(&parts, usize::from(max));
        serde_wasm_bindgen::to_value(&found).map_err(|e| JsError::new(&e.to_string()))
    }
}
