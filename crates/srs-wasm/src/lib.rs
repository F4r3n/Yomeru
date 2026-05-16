use srs_core::{filter_due, new_card, next_review_ms, review_card, ReviewRating, SrsCard};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());
}

/// Stateless SRS engine. All card state lives in JS/IndexedDB.
#[wasm_bindgen]
pub struct SrsEngine {}

#[wasm_bindgen]
impl SrsEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> SrsEngine {
        SrsEngine {}
    }

    /// Create a new SrsCard for a word being added.
    /// Returns the card as a JS object.
    pub fn new_card(&self, word: &str, now_ms: f64) -> Result<JsValue, JsError> {
        let card = new_card(word, now_ms);
        serde_wasm_bindgen::to_value(&card).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Apply a review rating (0-5) to a card. Returns the updated card.
    pub fn review_card(
        &self,
        card: JsValue,
        rating: u8,
        now_ms: f64,
    ) -> Result<JsValue, JsError> {
        let card: SrsCard = serde_wasm_bindgen::from_value(card)
            .map_err(|e| JsError::new(&format!("Invalid card: {e}")))?;
        let updated = review_card(card, ReviewRating::from_u8(rating), now_ms);
        serde_wasm_bindgen::to_value(&updated).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Filter a JS Array of cards to only those due at or before `now_ms`.
    pub fn filter_due(&self, cards: JsValue, now_ms: f64) -> Result<JsValue, JsError> {
        let cards: Vec<SrsCard> = serde_wasm_bindgen::from_value(cards)
            .map_err(|e| JsError::new(&format!("Invalid cards array: {e}")))?;
        let due: Vec<&SrsCard> = filter_due(&cards, now_ms);
        serde_wasm_bindgen::to_value(&due).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Returns the earliest due timestamp across all cards (or -1 if none).
    pub fn next_review_ms(&self, cards: JsValue) -> Result<f64, JsError> {
        let cards: Vec<SrsCard> = serde_wasm_bindgen::from_value(cards)
            .map_err(|e| JsError::new(&format!("Invalid cards array: {e}")))?;
        Ok(next_review_ms(&cards).unwrap_or(-1.0))
    }
}
