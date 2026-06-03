use dioxus::prelude::*;
use log::warn;

use crate::idb::{delete_card, get_all_cards};
use crate::srs::now_ms;
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardDirection, CardStatus, SrsCard};

#[component]
pub fn WordListTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    let mut filter = use_signal(String::new);
    let mut loading = use_signal(|| true);

    let reload = move || {
        spawn(async move {
            let all = get_all_cards().await.unwrap_or_default();
            let mut filtered: Vec<_> = all
                .into_iter()
                .filter(|c| matches!(c.status, CardStatus::Active))
                .collect();
            filtered.sort_by(|a, b| {
                a.word
                    .cmp(&b.word)
                    .then_with(|| a.direction.as_str().cmp(b.direction.as_str()))
            });
            cards.set(filtered);
            loading.set(false);
        });
    };

    // Reload on mount and whenever a sync lands.
    use_reload_on_sync(reload);

    let on_delete = move |word: String| {
        spawn(async move {
            if let Err(e) = delete_card(&word).await {
                warn!("delete_card({word}) failed: {e}");
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
    let filtered: Vec<_> = rows
        .into_iter()
        .filter(|c| filter_s.is_empty() || c.word.to_lowercase().contains(&filter_s))
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
                                    let word = c.word.clone();
                                    let due_label = format_due(c.due_ms, now);
                                    let due_class = if c.due_ms <= now { "badge due" } else { "badge" };
                                    let direction = match c.direction {
                                        CardDirection::Recognition => "Recognition",
                                        CardDirection::Recall => "Recall",
                                    };
                                    let state = format!("{:?}", c.state).to_lowercase();
                                    rsx! {
                                        tr {
                                            td { style: "padding-left: 16px; font-size: 15px;", "{c.word}" }
                                            td { span { class: "badge", "{direction}" } }
                                            td { class: "muted", "{state}" }
                                            td { span { class: "{due_class}", "{due_label}" } }
                                            td { style: "text-align: right; padding-right: 16px;",
                                                button {
                                                    class: "danger",
                                                    onclick: move |_| on_delete(word.clone()),
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
