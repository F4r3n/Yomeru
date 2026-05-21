pub mod dictionary;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_kanjidic_wasm() {
    console_error_panic_hook::set_once();
}

pub use dictionary::{init_from_bytes, lookup_many, lookup_one, KanjiDictionary};
