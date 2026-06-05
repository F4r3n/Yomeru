use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use jmdict_types::WordEntry;

use crate::components::pos_list;
use crate::dict::{frequency_label, lookup_by_sequence, preferred_headword, primary_reading};
use crate::idb::{delete_card, get_staging_cards, promote_card};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::SrsCard;

#[component]
pub fn NewWordsTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    let mut entries = use_signal(HashMap::<u32, WordEntry>::new);
    let mut expanded = use_signal(HashSet::<u32>::new);
    let mut loading = use_signal(|| true);
    let mut err = use_signal(|| Option::<String>::None);

    let reload = move || {
        spawn(async move {
            match get_staging_cards().await {
                Ok(c) => {
                    let deduped = unique_by_sequence(c);
                    let seqs: Vec<u32> = deduped.iter().map(|c| c.sequence).collect();
                    let looked_up = lookup_by_sequence(&seqs).await.unwrap_or_default();
                    let mut map = HashMap::with_capacity(deduped.len());
                    for (card, entry) in deduped.iter().zip(looked_up.iter()) {
                        if let Some(e) = entry {
                            map.insert(card.sequence, e.clone());
                        }
                    }
                    entries.set(map);
                    cards.set(deduped);
                    loading.set(false);
                }
                Err(e) => {
                    err.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    // Reload on mount and whenever a sync lands.
    use_reload_on_sync(reload);

    let promote_one = move |seq: u32| {
        spawn(async move {
            if let Err(e) = promote_card(seq).await {
                warn!("promote_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            reload();
        });
    };

    let reject_one = move |seq: u32| {
        spawn(async move {
            if let Err(e) = delete_card(seq).await {
                warn!("delete_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            reload();
        });
    };

    let promote_all = move |_| {
        spawn(async move {
            let staging = match get_staging_cards().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("get_staging_cards for promote_all failed: {e}");
                    return;
                }
            };
            let mut promoted = 0usize;
            for seq in unique_by_sequence(staging).into_iter().map(|c| c.sequence) {
                if let Err(e) = promote_card(seq).await {
                    warn!("promote_card(seq={seq}) in promote_all failed: {e}");
                    continue;
                }
                promoted += 1;
            }
            if promoted > 0 {
                schedule_sync();
            }
            reload();
        });
    };

    let count = cards.read().len();

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "New Words" }
                    div { class: "subtitle", "Review staged words before they enter the SRS queue." }
                }
                if count > 0 {
                    div { class: "actions",
                        span { class: "pill", "{count} staged" }
                        button { class: "primary", onclick: promote_all, "Promote all" }
                    }
                }
            }

            if *loading.read() {
                div { class: "loading", "Loading…" }
            } else if let Some(e) = err.read().clone() {
                div { class: "card error", "Failed: {e}" }
            } else if cards.read().is_empty() {
                div { class: "empty-state",
                    div { class: "glyph", "✦" }
                    div { class: "headline", "No staged words" }
                    div { class: "helper", "Add words from the Lookup tab to queue them here." }
                }
            } else {
                div { class: "col",
                    for card in cards.read().iter().cloned() {
                        {
                            let seq = card.sequence;
                            let entry = entries.read().get(&seq).cloned();
                            let label = entry
                                .as_ref()
                                .map(|e| preferred_headword(e).to_string())
                                .unwrap_or_else(|| format!("(seq {seq})"));
                            let reading = entry
                                .as_ref()
                                .map(|e| primary_reading(e).to_string())
                                .unwrap_or_default();
                            let show_reading = !reading.is_empty() && reading != label;
                            let freq = entry.as_ref().and_then(frequency_label);
                            let has_detail = entry.is_some();
                            let is_open = expanded.read().contains(&seq);
                            rsx! {
                                div { class: "card",
                                    div { class: "row", style: "justify-content: space-between; align-items: center;",
                                        div {
                                            class: if has_detail { "row clickable" } else { "row" },
                                            style: "align-items: baseline; gap: 8px;",
                                            onclick: move |_| {
                                                if has_detail {
                                                    expanded.with_mut(|s| if !s.remove(&seq) { s.insert(seq); });
                                                }
                                            },
                                            div { class: "headword", "{label}" }
                                            if show_reading {
                                                div { class: "reading", "{reading}" }
                                            }
                                            if let Some(f) = freq {
                                                span { class: "freq-badge", "{f}" }
                                            }
                                        }
                                        div { class: "row",
                                            button {
                                                class: "success",
                                                onclick: move |_| (promote_one)(seq),
                                                "Accept"
                                            }
                                            button {
                                                class: "danger",
                                                onclick: move |_| (reject_one)(seq),
                                                "Reject"
                                            }
                                        }
                                    }
                                    if is_open && let Some(e) = entry {
                                        EntryDetail { entry: e }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The senses/glosses of a staged entry, revealed inline so the user can see
/// what the word means before promoting it into the SRS queue.
#[component]
fn EntryDetail(entry: WordEntry) -> Element {
    let sense_count = entry.senses.len();
    rsx! {
        div { class: "entry-detail",
            for (si, sense) in entry.senses.iter().enumerate() {
                {
                    let needs_divider = si > 0;
                    rsx! {
                        if needs_divider {
                            hr { class: "divider" }
                        }
                        div {
                            if !sense.pos.is_empty() {
                                div { class: "pos", "{pos_list(&sense.pos)}" }
                            }
                            for gloss in sense.glosses.iter() {
                                div { class: "gloss",
                                    if sense_count > 1 {
                                        span { class: "muted", style: "margin-right: 4px;", "{si + 1}." }
                                    }
                                    "{gloss.text}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn unique_by_sequence(mut cards: Vec<SrsCard>) -> Vec<SrsCard> {
    let mut seen = std::collections::HashSet::new();
    cards.retain(|c| seen.insert(c.sequence));
    cards
}
