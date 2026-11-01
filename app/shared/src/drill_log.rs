//! Per-device count of how many drill sessions each word or kanji has been in.
//!
//! Lives in `localStorage`, deliberately outside the cards and the sync: it
//! only decides which batch of trouble cards the next drill picks, so losing
//! it (cleared site data, a new device) just restarts the rotation.

use std::collections::HashMap;

use dioxus::logger::tracing;
use gloo_storage::{LocalStorage, Storage};

/// Which deck a drill count belongs to; each keeps its own rotation.
#[derive(Clone, Copy)]
pub enum Deck {
    Words,
    Kanji,
}

impl Deck {
    fn key(self) -> &'static str {
        match self {
            Deck::Words => "drill_counts",
            Deck::Kanji => "kanji_drill_counts",
        }
    }
}

/// Sequence → number of drill sessions the card was answered in. Empty when
/// nothing was stored or storage is unavailable.
pub fn load(deck: Deck) -> HashMap<u32, u32> {
    LocalStorage::get(deck.key()).unwrap_or_default()
}

/// Count one more drill for `sequence`. Best-effort: a failed write only
/// costs the rotation order.
pub fn record(deck: Deck, sequence: u32) {
    let mut counts = load(deck);
    *counts.entry(sequence).or_insert(0) += 1;
    if let Err(e) = LocalStorage::set(deck.key(), &counts) {
        tracing::warn!("drill_log: saving drill counts failed: {e}");
    }
}
