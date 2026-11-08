use std::collections::HashMap;

use dioxus::prelude::*;

use crate::dict::{direction_label, lookup_by_sequence, preferred_headword, primary_reading};
use crate::icons::{Icon, IconName};
use crate::idb::{delete_card, demote_card, get_all_cards, reset_card};
use crate::romaji::PreparedQuery;
use crate::srs::now_ms;
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardStatus, SrsCard};
use srs_core::CardState;

/// Which words the list shows. A word matches when any of its direction
/// cards does; graduated cards never match Due/New/Review.
#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Due,
    New,
    Review,
}

impl Filter {
    fn matches(self, c: &SrsCard, now: f64) -> bool {
        let active = matches!(c.status, CardStatus::Active);
        match self {
            Filter::All => true,
            Filter::Due => active && c.due_ms <= now,
            Filter::New => active && c.state == CardState::New,
            Filter::Review => active && c.state == CardState::Review,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Alpha,
    AddedNewest,
    AddedOldest,
    NextDue,
}

impl Sort {
    const ALL: [Sort; 4] = [
        Sort::Alpha,
        Sort::AddedNewest,
        Sort::AddedOldest,
        Sort::NextDue,
    ];

    fn key(self) -> &'static str {
        match self {
            Sort::Alpha => "alpha",
            Sort::AddedNewest => "added-new",
            Sort::AddedOldest => "added-old",
            Sort::NextDue => "due",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Sort::Alpha => "A–Z",
            Sort::AddedNewest => "Newest added",
            Sort::AddedOldest => "Oldest added",
            Sort::NextDue => "Next due",
        }
    }

    fn from_key(k: &str) -> Sort {
        Sort::ALL
            .into_iter()
            .find(|s| s.key() == k)
            .unwrap_or(Sort::Alpha)
    }
}

/// When the word entered the list: its earliest-added direction.
fn word_added(cs: &[SrsCard]) -> f64 {
    cs.iter().map(|c| c.added_ms).fold(f64::INFINITY, f64::min)
}

/// Its soonest active review; graduated-only words sort last.
fn word_next_due(cs: &[SrsCard]) -> f64 {
    cs.iter()
        .filter(|c| matches!(c.status, CardStatus::Active))
        .map(|c| c.due_ms)
        .fold(f64::INFINITY, f64::min)
}

#[component]
pub fn WordListTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    // sequence -> displayed headword/reading, looked up from JMdict at load
    // time so filtering and rendering don't need an async dict hop per row.
    let mut headwords = use_signal(HashMap::<u32, String>::new);
    let mut readings = use_signal(HashMap::<u32, String>::new);
    let mut filter = use_signal(String::new);
    let mut state_filter = use_signal(|| Filter::All);
    let mut sort = use_signal(|| Sort::Alpha);
    let mut loading = use_signal(|| true);
    // The word whose Reset/Delete buttons are showing (phone layout only;
    // on wide screens they're always visible).
    let mut open = use_signal(|| None::<u32>);

    let reload = move || {
        spawn(async move {
            let all = get_all_cards().await.unwrap_or_default();
            let mut active: Vec<_> = all
                .into_iter()
                .filter(|c| {
                    !c.is_kanji() && matches!(c.status, CardStatus::Active | CardStatus::Graduated)
                })
                .collect();
            let mut seqs: Vec<u32> = active.iter().map(|c| c.sequence).collect();
            seqs.sort_unstable();
            seqs.dedup();
            let entries = lookup_by_sequence(&seqs).await.unwrap_or_default();
            let mut map: HashMap<u32, String> = HashMap::with_capacity(seqs.len());
            let mut reading_map: HashMap<u32, String> = HashMap::with_capacity(seqs.len());
            for (seq, entry) in seqs.iter().zip(entries.iter()) {
                if let Some(e) = entry {
                    map.insert(*seq, preferred_headword(e).to_string());
                    reading_map.insert(*seq, primary_reading(e).to_string());
                }
            }
            active.sort_by(|a, b| {
                let aw = map.get(&a.sequence).map(String::as_str).unwrap_or("");
                let bw = map.get(&b.sequence).map(String::as_str).unwrap_or("");
                aw.cmp(bw)
                    .then_with(|| a.direction.as_str().cmp(b.direction.as_str()))
            });
            headwords.set(map);
            readings.set(reading_map);
            cards.set(active);
            loading.set(false);
        });
    };

    // Reload on mount and whenever a sync lands.
    use_reload_on_sync(reload);

    let on_delete = move |seq: u32| {
        spawn(async move {
            let confirmed =
                crate::rt::confirm("Delete this word and its SRS progress? This can't be undone.")
                    .await;
            if !confirmed {
                return;
            }
            if let Err(e) = delete_card(seq).await {
                warn!("delete_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            reload();
        });
    };

    let on_reset = move |seq: u32| {
        spawn(async move {
            let confirmed =
                crate::rt::confirm("Reset this word's SRS progress? This can't be undone.").await;
            if !confirmed {
                return;
            }
            if let Err(e) = reset_card(seq, now_ms()).await {
                warn!("reset_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            reload();
        });
    };

    let on_demote = move |seq: u32| {
        spawn(async move {
            let confirmed =
                crate::rt::confirm("Move this word back to New Words? Its SRS progress is reset.")
                    .await;
            if !confirmed {
                return;
            }
            if let Err(e) = demote_card(seq, now_ms()).await {
                warn!("demote_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            reload();
        });
    };

    let now = now_ms();
    let rows = cards.read().clone();
    // One entry per word, both directions together, in the load order
    // (sorted by headword).
    let mut words: Vec<(u32, Vec<SrsCard>)> = Vec::new();
    for c in rows {
        match words.last_mut() {
            Some((seq, cs)) if *seq == c.sequence => cs.push(c),
            _ => words.push((c.sequence, vec![c])),
        }
    }
    let total = words.len();
    let is_graduated =
        |cs: &[SrsCard]| cs.iter().all(|c| matches!(c.status, CardStatus::Graduated));
    let graduated_count = words.iter().filter(|(_, cs)| is_graduated(cs)).count();
    let active_count = total - graduated_count;
    let filter_s = filter.read().to_lowercase();
    let heads = headwords.read().clone();
    let reads = readings.read().clone();
    let query = PreparedQuery::new(&filter_s);
    let state_f = *state_filter.read();
    let sort_by = *sort.read();
    let mut filtered: Vec<_> = words
        .into_iter()
        .filter(|(_, cs)| cs.iter().any(|c| state_f.matches(c, now)))
        .filter(|(seq, _)| {
            if query.is_empty() {
                return true;
            }
            let head = heads.get(seq).map(String::as_str).unwrap_or("");
            let reading = reads.get(seq).map(String::as_str).unwrap_or("");
            query.matches(head, reading)
        })
        .collect();
    // Stable sorts: ties keep the headword order the list was loaded in.
    match sort_by {
        Sort::Alpha => {}
        Sort::AddedNewest => filtered.sort_by(|a, b| word_added(&b.1).total_cmp(&word_added(&a.1))),
        Sort::AddedOldest => filtered.sort_by(|a, b| word_added(&a.1).total_cmp(&word_added(&b.1))),
        Sort::NextDue => {
            filtered.sort_by(|a, b| word_next_due(&a.1).total_cmp(&word_next_due(&b.1)))
        }
    }
    let narrowed = !filter_s.is_empty() || state_f != Filter::All;
    // Due *reviews* (cards), as on the Review tab.
    let due_count = filtered
        .iter()
        .flat_map(|(_, cs)| cs)
        .filter(|c| matches!(c.status, CardStatus::Active) && c.due_ms <= now)
        .count();
    let visible = filtered.len();
    let open_seq = *open.read();

    rsx! {
        div {
            div { class: "page-header",
                div { class: "actions",
                    span { class: "pill", "{active_count} active" }
                    span { class: "pill", "{graduated_count} graduated" }
                    if narrowed {
                        span { class: "pill", "{visible} match · {due_count} due" }
                    } else {
                        span { class: "pill", "{due_count} due" }
                    }
                }
            }

            div { class: "toolbar",
                input {
                    r#type: "search",
                    placeholder: "Filter by word, reading, or romaji…",
                    value: "{filter}",
                    oninput: move |e| filter.set(e.value()),
                }
            }

            div { class: "toolbar list-controls",
                div { class: "seg-tabs",
                    for (f, label) in [(Filter::All, "All"), (Filter::Due, "Due"), (Filter::New, "New"), (Filter::Review, "Review")] {
                        button {
                            class: if state_f == f { "active" } else { "" },
                            onclick: move |_| state_filter.set(f),
                            "{label}"
                        }
                    }
                }
                select {
                    class: "sort-select",
                    "aria-label": "Sort",
                    value: "{sort_by.key()}",
                    onchange: move |e| sort.set(Sort::from_key(&e.value())),
                    for s in Sort::ALL {
                        option { value: "{s.key()}", selected: s == sort_by, "{s.label()}" }
                    }
                }
            }

            if *loading.read() {
                div { class: "loading", "Loading…" }
            } else if total == 0 {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::List, size: 44 } }
                    div { class: "headline", "No active cards yet" }
                    div { class: "helper", "Promote staged words from the New Words tab to start reviewing." }
                }
            } else if filtered.is_empty() {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Search, size: 44 } }
                    div { class: "headline", "No matches" }
                    if filter_s.is_empty() {
                        div { class: "helper", "No words match this filter." }
                    } else {
                        div { class: "helper", "Nothing here contains 「{filter_s}」." }
                    }
                }
            } else {
                div { class: "card word-list",
                    for (seq, cs) in filtered {
                        {
                            let label = heads
                                .get(&seq)
                                .cloned()
                                .unwrap_or_else(|| format!("(seq {seq})"));
                            let reading = reads.get(&seq).cloned().unwrap_or_default();
                            let show_reading = !reading.is_empty() && reading != label;
                            let row_class = if open_seq == Some(seq) { "word-item open" } else { "word-item" };
                            rsx! {
                                div { key: "{seq}", class: "{row_class}",
                                    onclick: move |_| {
                                        let next = if open_seq == Some(seq) { None } else { Some(seq) };
                                        open.set(next);
                                    },
                                    div { class: "wi-word",
                                        span { class: "wi-head", "{label}" }
                                        if show_reading {
                                            span { class: "wi-reading", "{reading}" }
                                        }
                                    }
                                    div { class: "wi-dirs",
                                        for c in cs {
                                            {
                                                let graduated = matches!(c.status, CardStatus::Graduated);
                                                let due_label = if graduated {
                                                    "graduated".to_string()
                                                } else {
                                                    format_due(c.due_ms, now)
                                                };
                                                let due_class = if !graduated && c.due_ms <= now {
                                                    "badge due"
                                                } else {
                                                    "badge"
                                                };
                                                let state = format!("{:?}", c.state).to_lowercase();
                                                rsx! {
                                                    div { class: "wi-dir",
                                                        span { class: "wi-dir-name", "{direction_label(c.direction)}" }
                                                        span { class: "wi-state", "{state}" }
                                                        span { class: "{due_class}", "{due_label}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    div { class: "wi-actions",
                                        button { class: "outline",
                                            title: "Back to New Words",
                                            onclick: move |e| { e.stop_propagation(); on_demote(seq) },
                                            "To New Words"
                                        }
                                        button { class: "outline",
                                            onclick: move |e| { e.stop_propagation(); on_reset(seq) },
                                            "Reset"
                                        }
                                        button { class: "danger",
                                            onclick: move |e| { e.stop_propagation(); on_delete(seq) },
                                            "Delete"
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
}

fn format_due(due_ms: f64, now: f64) -> String {
    let delta_ms = due_ms - now;
    if delta_ms <= 0.0 {
        return "due".into();
    }
    let mins = (delta_ms / 60_000.0).round() as i64;
    if mins < 60 {
        return format!("{mins} min");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours} hr");
    }
    let days = hours / 24;
    format!("{days} d")
}
