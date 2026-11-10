use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use jmdict_types::WordEntry;

use crate::components::pos_list;
use crate::dict::{frequency_label, lookup_by_sequence, preferred_headword, primary_reading};
use crate::icons::{Icon, IconName};
use crate::idb::{delete_card, get_staging_cards, promote_card};
use crate::romaji::PreparedQuery;
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::SrsCard;

#[component]
pub fn NewWordsTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    let mut entries = use_signal(HashMap::<u32, WordEntry>::new);
    let mut expanded = use_signal(HashSet::<u32>::new);
    let mut loading = use_signal(|| true);
    let mut err = use_signal(|| Option::<String>::None);
    let mut filter = use_signal(String::new);
    // Generation of the last reload started / last reload whose result was
    // applied. A reload only applies its result if no newer one started in
    // the meantime, so overlapping reloads can't resurrect stale data.
    // Read with `peek` only: these must never trigger a re-render.
    let load_gen = use_signal(|| 0u64);
    let applied_gen = use_signal(|| 0u64);

    let reload = move || {
        let mut load_gen = load_gen;
        let mut applied_gen = applied_gen;
        let my_gen = *load_gen.peek() + 1;
        load_gen.set(my_gen);
        spawn(async move {
            let result = get_staging_cards().await;
            let result = match result {
                Ok(c) => {
                    let deduped = unique_by_sequence(c);
                    let seqs: Vec<u32> = deduped.iter().map(|c| c.sequence).collect();
                    let looked_up = match lookup_by_sequence(&seqs).await {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(
                                "lookup_by_sequence({} staged cards) failed: {e}",
                                seqs.len()
                            );
                            Vec::new()
                        }
                    };
                    Ok((deduped, looked_up))
                }
                Err(e) => Err(e),
            };
            if *load_gen.peek() != my_gen {
                return; // superseded by a newer reload
            }
            applied_gen.set(my_gen);
            match result {
                Ok((deduped, looked_up)) => {
                    let map: HashMap<u32, WordEntry> = deduped
                        .iter()
                        .zip(looked_up)
                        .filter_map(|(card, entry)| Some((card.sequence, entry?)))
                        .collect();
                    entries.set(map);
                    cards.set(deduped);
                    err.set(None);
                }
                Err(e) => err.set(Some(e)),
            }
            loading.set(false);
        });
    };

    // Reload on mount and whenever a sync lands.
    use_reload_on_sync(reload);

    // Drop a promoted/rejected card from the local list instead of
    // re-fetching everything. If a reload is still in flight it may have read
    // the store before our write, so restart it to get a fresh snapshot.
    let remove_local = move |seq: u32| {
        let (mut cards, mut entries, mut expanded) = (cards, entries, expanded);
        cards.with_mut(|c| c.retain(|c| c.sequence != seq));
        entries.with_mut(|m| {
            m.remove(&seq);
        });
        expanded.with_mut(|s| {
            s.remove(&seq);
        });
        if *load_gen.peek() != *applied_gen.peek() {
            reload();
        }
    };

    let promote_one = move |seq: u32| {
        spawn(async move {
            if let Err(e) = promote_card(seq).await {
                warn!("promote_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            remove_local(seq);
        });
    };

    let reject_one = move |seq: u32| {
        spawn(async move {
            if let Err(e) = delete_card(seq).await {
                warn!("delete_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            remove_local(seq);
        });
    };

    let cards_r = cards.read();
    let entries_r = entries.read();
    let count = cards_r.len();
    let filter_s = filter.read().to_lowercase();
    let query = PreparedQuery::new(&filter_s);
    let filtered: Vec<&SrsCard> = cards_r
        .iter()
        .filter(|c| {
            query.is_empty()
                || entries_r
                    .get(&c.sequence)
                    .is_some_and(|e| query.matches(preferred_headword(e), primary_reading(e)))
        })
        .collect();

    rsx! {
        div {
            if count > 0 {
                div { class: "page-header",
                    div { class: "actions",
                        span { class: "pill", "{count} staged" }
                    }
                }
            }

            if count > 0 {
                div { class: "toolbar",
                    input {
                        r#type: "search",
                        placeholder: "Filter by word, reading, or romaji…",
                        // No `value`: echoing it back breaks Japanese IME composition on
                        // Android (see the lookup search box). The filter starts empty.
                        oninput: move |e| filter.set(e.value()),
                    }
                }
            }

            if *loading.read() {
                div { class: "loading", "Loading…" }
            } else if let Some(e) = err.read().clone() {
                div { class: "card error", "Failed: {e}" }
            } else if cards_r.is_empty() {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Sparkles, size: 44 } }
                    div { class: "headline", "No staged words" }
                    div { class: "helper", "Add words from the Lookup tab to queue them here." }
                }
            } else if filtered.is_empty() {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Search, size: 44 } }
                    div { class: "headline", "No matches" }
                    div { class: "helper", "Nothing staged contains 「{filter_s}」." }
                }
            } else {
                div { class: "col",
                    for card in filtered.iter().copied() {
                        {
                            let seq = card.sequence;
                            let priority = card.priority;
                            let entry = entries_r.get(&seq);
                            let label = entry
                                .map(|e| preferred_headword(e).to_string())
                                .unwrap_or_else(|| format!("(seq {seq})"));
                            let reading = entry.map(primary_reading).unwrap_or_default();
                            let show_reading = !reading.is_empty() && reading != label;
                            let freq = entry.and_then(frequency_label);
                            let is_open = expanded.read().contains(&seq);
                            let item_class = if is_open { "card staged-item open" } else { "card staged-item" };
                            rsx! {
                                // Tap toggles the meaning; on phones it also
                                // reveals Accept/Reject (see .staged-item CSS).
                                div { class: "{item_class}",
                                    onclick: move |_| {
                                        expanded.with_mut(|s| if !s.remove(&seq) { s.insert(seq); });
                                    },
                                    div { class: "si-head",
                                        div { class: "headword", "{label}" }
                                        if show_reading {
                                            div { class: "reading", "{reading}" }
                                        }
                                        if let Some(f) = freq {
                                            span { class: "freq-badge", "{f}" }
                                        }
                                        if priority > 0 {
                                            span { class: "freq-badge", "★ {priority}" }
                                        }
                                    }
                                    div { class: "si-actions",
                                        button {
                                            class: "success",
                                            onclick: move |e| { e.stop_propagation(); (promote_one)(seq) },
                                            "Accept"
                                        }
                                        button {
                                            class: "danger",
                                            onclick: move |e| { e.stop_propagation(); (reject_one)(seq) },
                                            "Reject"
                                        }
                                    }
                                    if is_open && let Some(e) = entry {
                                        EntryDetail { entry: e.clone() }
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
    let mut seen = HashSet::with_capacity(cards.len());
    cards.retain(|c| seen.insert(c.sequence));
    cards
}
