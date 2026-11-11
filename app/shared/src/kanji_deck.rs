//! The kanji deck from the UI's side: [`yomeru_core::kanji_deck`] with the
//! platform taken from Dioxus context.

use std::collections::HashMap;

use dioxus::prelude::consume_context;

pub use yomeru_core::kanji_deck::*;

use crate::platform::Platform;

/// See [`yomeru_core::kanji_deck::ensure_kanji_cards`].
pub async fn ensure_kanji_cards() -> Result<HashMap<char, Vec<StudiedWord>>, String> {
    yomeru_core::kanji_deck::ensure_kanji_cards(&consume_context::<Platform>()).await
}
