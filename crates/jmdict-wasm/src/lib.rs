pub mod dictionary;
pub mod lookup;
#[cfg(test)]
mod tests;

pub use dictionary::init_for_testing;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_jmdict_wasm() {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());
}

pub use dictionary::Dictionary;

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
