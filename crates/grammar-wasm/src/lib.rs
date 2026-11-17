//! Thin `#[wasm_bindgen]` shim over `grammar-core`, for the extension's
//! background script. It loads the `data/grammar.bin` shipped with the
//! extension.

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

/// What the hover popup shows of an entry: a glance, not the explanation.
#[derive(Serialize)]
struct PopupEntry {
    id: String,
    pattern: String,
    jlpt: String,
    meaning: String,
    formation: Vec<String>,
    examples: Vec<grammar_core::Example>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PopupMatch {
    entries: Vec<PopupEntry>,
    match_len: usize,
}

#[wasm_bindgen]
impl GrammarDictionary {
    /// Loads `grammar.bin` bytes. Fails on a file of another format
    /// version.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<GrammarDictionary, JsError> {
        let inner = GrammarDict::from_bytes(bytes).map_err(|e| JsError::new(&format!("{e:#}")))?;
        Ok(GrammarDictionary { inner })
    }

    /// The full entries grammar cards with these `sequence`s review, `null`
    /// for a point that left the dictionary (`GrammarDict::by_seq`).
    pub fn by_seq(&self, sequences: &[u32]) -> Result<JsValue, JsError> {
        let found: Vec<_> = sequences.iter().map(|&s| self.inner.by_seq(s)).collect();
        to_js(&found)
    }

    /// Entries whose pattern or a form is exactly `text`.
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

    /// The hover popup's lookup (`GrammarDict::hover_match`):
    /// `{entries, matchLen}` of summaries with at most `max_examples`
    /// examples, or `null`.
    pub fn popup_lookup(
        &self,
        text: &str,
        max_chars: usize,
        max_examples: usize,
    ) -> Result<JsValue, JsError> {
        let Some((entries, match_len)) = self.inner.hover_match(text, max_chars) else {
            return Ok(JsValue::NULL);
        };
        let entries: Vec<PopupEntry> = entries
            .into_iter()
            .map(|e| PopupEntry {
                id: e.id,
                pattern: e.pattern,
                jlpt: e.jlpt,
                meaning: e.meaning,
                formation: e.formation,
                examples: e.examples.into_iter().take(max_examples).collect(),
            })
            .collect();
        to_js(&PopupMatch { entries, match_len })
    }

    /// Entries with a pattern or form starting with `prefix`, at most `max`.
    pub fn lookup_prefix(&self, prefix: &str, max: usize) -> Result<JsValue, JsError> {
        to_js(&self.inner.lookup_prefix(prefix, max))
    }

    /// What the Lookup page's search shows for `text` (`GrammarDict::search`).
    pub fn search(&self, text: &str, max: usize) -> Result<JsValue, JsError> {
        to_js(&self.inner.search(text, max))
    }
}

/// The `sequence` of the review card for grammar point `id`
/// (`grammar_core::grammar_seq`).
#[wasm_bindgen]
pub fn grammar_seq(id: &str) -> u32 {
    grammar_core::grammar_seq(id)
}
