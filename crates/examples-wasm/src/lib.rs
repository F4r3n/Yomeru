//! Thin `#[wasm_bindgen]` shim over `examples-core`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_examples_wasm() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct ExamplesDict {}

#[wasm_bindgen]
impl ExamplesDict {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<ExamplesDict, JsError> {
        examples_core::init_from_bytes(bytes)
            .map_err(|e| JsError::new(&format!("Failed to load examples: {e}")))?;
        Ok(ExamplesDict {})
    }

    /// Returns up to `max` example sentences for `headword` as `{japanese, english}[]`.
    pub fn lookup(&self, headword: &str, max: u8) -> Result<JsValue, JsError> {
        let entries = examples_core::lookup(headword, max as usize);
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))
    }
}
