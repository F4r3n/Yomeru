//! Thin `#[wasm_bindgen]` shim over `grammar-core`, for the extension's
//! background script. The bytes it loads are the user's own `grammar.bin`,
//! converted on their device: this module ships no dictionary data.

use grammar_core::GrammarDict;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_grammar_wasm() {
    console_error_panic_hook::set_once();
}

fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    // Plain objects rather than JS Maps, so the result survives
    // `runtime.sendMessage` to the content script.
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&ser)
        .map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub struct GrammarDictionary {
    inner: GrammarDict,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Match<'a> {
    entries: &'a [grammar_core::GrammarEntry],
    /// Chars of the text the match covers, for highlighting.
    match_len: usize,
}

/// What the hover popup shows of an entry: a glance, not the sources.
#[derive(Serialize)]
struct PopupEntry {
    term: String,
    reading: String,
    jlpt: String,
    meaning: String,
    structure: String,
    examples: Vec<grammar_core::Example>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PopupMatch {
    entries: Vec<PopupEntry>,
    match_len: usize,
}

/// Shortest match and headword the popup offers: one-char ones are
/// particles and endings (は, た) that would come up on almost every word.
const POPUP_MIN_CHARS: usize = 2;

#[wasm_bindgen]
impl GrammarDictionary {
    /// Loads `grammar.bin` bytes. Fails on a file of another format
    /// version, with a message asking to import it again.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<GrammarDictionary, JsError> {
        let inner = GrammarDict::from_bytes(bytes).map_err(|e| JsError::new(&format!("{e:#}")))?;
        Ok(GrammarDictionary { inner })
    }

    /// `{title, revision, attribution, url, indexUrl, downloadUrl}`.
    pub fn meta(&self) -> Result<JsValue, JsError> {
        to_js(self.inner.meta())
    }

    /// Entries whose term or reading is exactly `text`.
    pub fn lookup(&self, text: &str) -> Result<JsValue, JsError> {
        to_js(&self.inner.lookup(text))
    }

    /// `{entries, matchLen}` for the longest grammar point `text` starts
    /// with, or `null`.
    pub fn lookup_longest_match(&self, text: &str, max_chars: usize) -> Result<JsValue, JsError> {
        match self.inner.lookup_longest_match(text, max_chars) {
            Some((entries, match_len)) => to_js(&Match {
                entries: &entries,
                match_len,
            }),
            None => Ok(JsValue::NULL),
        }
    }

    /// The hover popup's lookup: like [`Self::lookup_longest_match`], but
    /// `{entries, matchLen}` holds summaries (whole meaning, at most
    /// `max_examples` examples), and a match under two chars is `null`.
    pub fn popup_lookup(
        &self,
        text: &str,
        max_chars: usize,
        max_examples: usize,
    ) -> Result<JsValue, JsError> {
        let Some((entries, match_len)) = self.inner.lookup_longest_match(text, max_chars) else {
            return Ok(JsValue::NULL);
        };
        let entries: Vec<PopupEntry> = entries
            .into_iter()
            .filter(|e| e.term.chars().count() >= POPUP_MIN_CHARS)
            .map(|e| PopupEntry {
                meaning: e.full_meaning(),
                examples: e.preview_examples(max_examples),
                term: e.term,
                reading: e.reading,
                jlpt: e.jlpt,
                structure: e.structure,
            })
            .collect();
        if match_len < POPUP_MIN_CHARS || entries.is_empty() {
            return Ok(JsValue::NULL);
        }
        to_js(&PopupMatch { entries, match_len })
    }

    /// Entries whose term or reading starts with `prefix`, at most `max`.
    pub fn lookup_prefix(&self, prefix: &str, max: usize) -> Result<JsValue, JsError> {
        to_js(&self.inner.lookup_prefix(prefix, max))
    }
}
