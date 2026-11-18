//! Thin `#[wasm_bindgen]` shim over `grammar-core`, for the extension's
//! background script. It loads the `data/grammar.bin` shipped with the
//! extension.

use grammar_core::{GrammarDict, Vocabulary};
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

/// The answer check's words, from the JMdict module through two JS
/// callbacks: the two WASM modules don't share memory.
struct JsVocabulary<'a> {
    is_verb: &'a js_sys::Function,
    word_len_at: &'a js_sys::Function,
}

impl Vocabulary for JsVocabulary<'_> {
    fn is_verb(&self, base: &str) -> bool {
        self.is_verb
            .call1(&JsValue::NULL, &JsValue::from_str(base))
            .is_ok_and(|v| v.as_bool() == Some(true))
    }

    fn word_len_at(&self, text: &str, char_offset: usize) -> usize {
        let len = self
            .word_len_at
            .call2(
                &JsValue::NULL,
                &JsValue::from_str(text),
                &JsValue::from(char_offset as u32),
            )
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        // A JS number: whole and at least 1, or the check's ranges break.
        if len.is_finite() && len >= 1.0 {
            len as usize
        } else {
            1
        }
    }
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
    /// The entry's number, the `item` of its review card.
    seq: u32,
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
                seq: e.seq,
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

    /// The production answer check (`GrammarDict::check_answer`) of
    /// `answer` for the point `id`: a `Verdict` (`{kind, …}`).
    /// `is_verb(base) -> bool` and `word_len_at(text, charOffset) -> number`
    /// answer from JMdict.
    pub fn check_answer(
        &self,
        id: &str,
        answer: &str,
        is_verb: &js_sys::Function,
        word_len_at: &js_sys::Function,
    ) -> Result<JsValue, JsError> {
        let vocab = JsVocabulary {
            is_verb,
            word_len_at,
        };
        to_js(&self.inner.check_answer(id, answer, &vocab))
    }

    /// What the Lookup page's search shows for `text` (`GrammarDict::search`).
    pub fn search(&self, text: &str, max: usize) -> Result<JsValue, JsError> {
        to_js(&self.inner.search(text, max))
    }
}
