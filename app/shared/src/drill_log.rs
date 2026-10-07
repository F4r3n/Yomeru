//! Per-device count of how many drill sessions each word has been in.
//!
//! Lives in `localStorage`, deliberately outside the cards and the sync: it
//! only decides which batch of trouble words the next drill picks, so losing
//! it (cleared site data, a new device) just restarts the rotation.

use std::collections::HashMap;

use dioxus::logger::tracing;
use gloo_storage::{LocalStorage, Storage};

const DRILL_LOG_KEY: &str = "drill_counts";

/// Sequence → number of drill sessions the word was answered in. Empty when
/// nothing was stored or storage is unavailable.
pub fn load() -> HashMap<u32, u32> {
    LocalStorage::get(DRILL_LOG_KEY).unwrap_or_default()
}

/// Count one more drill for `sequence`. Best-effort: a failed write only
/// costs the rotation order.
pub fn record(sequence: u32) {
    let mut counts = load();
    *counts.entry(sequence).or_insert(0) += 1;
    if let Err(e) = LocalStorage::set(DRILL_LOG_KEY, &counts) {
        tracing::warn!("drill_log: saving drill counts failed: {e}");
    }
}
