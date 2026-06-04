use std::collections::HashMap;

use dioxus::prelude::*;

use crate::dict::{lookup_by_sequence, preferred_headword};
use crate::idb::{delete_card, get_all_cards};
use crate::srs::now_ms;
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardDirection, CardStatus, SrsCard};

#[component]
pub fn WordListTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    // sequence -> displayed headword, looked up from JMdict at load time so
    // filtering and rendering don't need an async dict hop per row.
    let mut headwords = use_signal(HashMap::<u32, String>::new);
    let mut filter = use_signal(String::new);
    let mut loading = use_signal(|| true);

    let reload = move || {
        spawn(async move {
            let all = get_all_cards().await.unwrap_or_default();
            let mut active: Vec<_> = all
                .into_iter()
                .filter(|c| matches!(c.status, CardStatus::Active))
                .collect();
            let mut seqs: Vec<u32> = active.iter().map(|c| c.sequence).collect();
            seqs.sort_unstable();
            seqs.dedup();
            let entries = lookup_by_sequence(&seqs).await.unwrap_or_default();
            let mut map: HashMap<u32, String> = HashMap::with_capacity(seqs.len());
            for (seq, entry) in seqs.iter().zip(entries.iter()) {
                if let Some(e) = entry {
                    map.insert(*seq, preferred_headword(e).to_string());
                }
            }
            active.sort_by(|a, b| {
                let aw = map.get(&a.sequence).map(String::as_str).unwrap_or("");
                let bw = map.get(&b.sequence).map(String::as_str).unwrap_or("");
                aw.cmp(bw)
                    .then_with(|| a.direction.as_str().cmp(b.direction.as_str()))
            });
            headwords.set(map);
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

    let now = now_ms();
    let rows = cards.read().clone();
    let total = rows.len();
    let filter_s = filter.read().to_lowercase();
    let heads = headwords.read().clone();
    let filtered: Vec<_> = rows
        .into_iter()
        .filter(|c| {
            if filter_s.is_empty() {
                return true;
            }
            heads
                .get(&c.sequence)
                .map(|w| w.to_lowercase().contains(&filter_s))
                .unwrap_or(false)
        })
        .collect();
    let due_count = filtered.iter().filter(|c| c.due_ms <= now).count();
    let visible = filtered.len();

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "Word List" }
                    div { class: "subtitle", "Active SRS cards across both review directions." }
                }
                div { class: "actions",
                    span { class: "pill", "{total} active" }
                }
            }

            div { class: "toolbar",
                input {
                    r#type: "search",
                    placeholder: "Filter by word…",
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
                                    let due_label = format_due(c.due_ms, now);
                                    let due_class = if c.due_ms <= now { "badge due" } else { "badge" };
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
