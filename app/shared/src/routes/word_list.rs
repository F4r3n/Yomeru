use dioxus::prelude::*;

use crate::idb::{delete_card, get_all_cards};
use crate::srs::now_ms;
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

    use_effect(move || reload());

    let on_delete = move |word: String| {
        spawn(async move {
            let _ = delete_card(&word).await;
            reload();
        });
    };

    let now = now_ms();
    let rows = cards.read().clone();

    rsx! {
        div { class: "col",
            input {
                r#type: "search",
                placeholder: "Filter words…",
                value: "{filter}",
                oninput: move |e| filter.set(e.value()),
            }
            if *loading.read() {
                div { class: "loading", "Loading…" }
            } else if rows.is_empty() {
                div { class: "empty", "No active cards yet." }
            } else {
                table {
                    thead {
                        tr {
                            th { "Word" }
                            th { "Direction" }
                            th { "State" }
                            th { "Due" }
                            th { "" }
                        }
                    }
                    tbody {
                        for c in rows.into_iter().filter(|c| {
                            let f = filter.read().to_lowercase();
                            f.is_empty() || c.word.to_lowercase().contains(&f)
                        }) {
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
                                        td { "{c.word}" }
                                        td { "{direction}" }
                                        td { "{state}" }
                                        td { span { class: "{due_class}", "{due_label}" } }
                                        td {
                                            button {
                                                class: "danger",
                                                onclick: move |_| (on_delete.clone())(word.clone()),
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
