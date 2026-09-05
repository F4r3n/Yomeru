use std::collections::HashMap;

use dioxus::prelude::*;

use crate::dict::{lookup_by_sequence, preferred_headword, primary_reading};
use crate::idb::{delete_card, get_all_cards, reset_card};
use crate::romaji::matches_query;
use crate::srs::now_ms;
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardDirection, CardStatus, SrsCard};

#[component]
pub fn WordListTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    // sequence -> displayed headword/reading, looked up from JMdict at load
    // time so filtering and rendering don't need an async dict hop per row.
    let mut headwords = use_signal(HashMap::<u32, String>::new);
    let mut readings = use_signal(HashMap::<u32, String>::new);
    let mut filter = use_signal(String::new);
    let mut loading = use_signal(|| true);

    let reload = move || {
        spawn(async move {
            let all = get_all_cards().await.unwrap_or_default();
            let mut active: Vec<_> = all
                .into_iter()
                .filter(|c| matches!(c.status, CardStatus::Active | CardStatus::Graduated))
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
            let confirmed = web_sys::window()
                .and_then(|w| {
                    w.confirm_with_message("Reset this word's SRS progress? This can't be undone.")
                        .ok()
                })
                .unwrap_or(false);
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

    let now = now_ms();
    let rows = cards.read().clone();
    let total = rows.len();
    let active_count = rows
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Active))
        .count();
    let graduated_count = rows
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Graduated))
        .count();
    let filter_s = filter.read().to_lowercase();
    let heads = headwords.read().clone();
    let reads = readings.read().clone();
    let filtered: Vec<_> = rows
        .into_iter()
        .filter(|c| {
            if filter_s.is_empty() {
                return true;
            }
            let head = heads.get(&c.sequence).map(String::as_str).unwrap_or("");
            let reading = reads.get(&c.sequence).map(String::as_str).unwrap_or("");
            matches_query(head, reading, &filter_s)
        })
        .collect();
    let due_count = filtered
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Active) && c.due_ms <= now)
        .count();
    let visible = filtered.len();

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "Word List" }
                    div { class: "subtitle", "Active SRS cards across both review directions." }
                }
                div { class: "actions",
                    span { class: "pill", "{active_count} active" }
                    span { class: "pill", "{graduated_count} graduated" }
                }
            }

            div { class: "toolbar",
                input {
                    r#type: "search",
                    placeholder: "Filter by word, reading, or romaji…",
                    value: "{filter}",
                    oninput: move |e| filter.set(e.value()),
                }
                span { class: "count",
                    if filter_s.is_empty() {
                        "{due_count} due now"
                    } else {
                        "{visible} match · {due_count} due"
                    }
                }
            }

            if *loading.read() {
                div { class: "loading", "Loading…" }
            } else if total == 0 {
                div { class: "empty-state",
                    div { class: "glyph", "≡" }
                    div { class: "headline", "No active cards yet" }
                    div { class: "helper", "Promote staged words from the New Words tab to start reviewing." }
                }
            } else if filtered.is_empty() {
                div { class: "empty-state",
                    div { class: "glyph", "⌕" }
                    div { class: "headline", "No matches" }
                    div { class: "helper", "Nothing in your list contains 「{filter_s}」." }
                }
            } else {
                div { class: "card table-card",
                    table {
                        thead {
                            tr {
                                th { style: "padding-left: 16px;", "Word" }
                                th { "Direction" }
                                th { "State" }
                                th { "Due" }
                                th { style: "text-align: right; padding-right: 16px;", "" }
                            }
                        }
                        tbody {
                            for c in filtered {
                                {
                                    let seq = c.sequence;
                                    let label = heads
                                        .get(&seq)
                                        .cloned()
                                        .unwrap_or_else(|| format!("(seq {seq})"));
                                    let graduated = matches!(c.status, CardStatus::Graduated);
                                    let due_label = if graduated {
                                        "graduated".to_string()
                                    } else {
                                        format_due(c.due_ms, now)
                                    };
                                    let due_class = if graduated {
                                        "badge"
                                    } else if c.due_ms <= now {
                                        "badge due"
                                    } else {
                                        "badge"
                                    };
                                    let direction = match c.direction {
                                        CardDirection::Recognition => "Recognition",
                                        CardDirection::Recall => "Recall",
                                    };
                                    let state = format!("{:?}", c.state).to_lowercase();
                                    rsx! {
                                        tr {
                                            td { style: "padding-left: 16px; font-size: 15px;", "{label}" }
                                            td { span { class: "badge", "{direction}" } }
                                            td { class: "muted", "{state}" }
                                            td { span { class: "{due_class}", "{due_label}" } }
                                            td { style: "text-align: right; padding-right: 16px;",
                                                button {
                                                    class: "secondary",
                                                    onclick: move |_| on_reset(seq),
                                                    "Reset"
                                                }
                                                button {
                                                    class: "danger",
                                                    onclick: move |_| on_delete(seq),
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
