use dioxus::prelude::*;
use log::warn;

use crate::idb::{delete_card, get_staging_cards, promote_card};
use crate::sync::schedule_sync;
use crate::types::SrsCard;

#[component]
pub fn NewWordsTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    let mut loading = use_signal(|| true);
    let mut err = use_signal(|| Option::<String>::None);

    let reload = move || {
        spawn(async move {
            match get_staging_cards().await {
                Ok(c) => {
                    cards.set(unique_by_word(c));
                    loading.set(false);
                }
                Err(e) => {
                    err.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    use_effect(move || reload());

    let promote_one = move |word: String| {
        spawn(async move {
            if let Err(e) = promote_card(&word).await {
                warn!("promote_card({word}) failed: {e}");
                return;
            }
            schedule_sync();
            reload();
        });
    };

    let reject_one = move |word: String| {
        spawn(async move {
            if let Err(e) = delete_card(&word).await {
                warn!("delete_card({word}) failed: {e}");
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
            for w in unique_by_word(staging).into_iter().map(|c| c.word) {
                if let Err(e) = promote_card(&w).await {
                    warn!("promote_card({w}) in promote_all failed: {e}");
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
                            let word = card.word.clone();
                            let word_a = word.clone();
                            let word_b = word.clone();
                            rsx! {
                                div { class: "card row", style: "justify-content: space-between; align-items: center;",
                                    div { class: "headword", "{word}" }
                                    div { class: "row",
                                        button {
                                            class: "success",
                                            onclick: move |_| (promote_one.clone())(word_a.clone()),
                                            "Accept"
                                        }
                                        button {
                                            class: "danger",
                                            onclick: move |_| (reject_one.clone())(word_b.clone()),
                                            "Reject"
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

fn unique_by_word(mut cards: Vec<SrsCard>) -> Vec<SrsCard> {
    let mut seen = std::collections::HashSet::new();
    cards.retain(|c| seen.insert(c.word.clone()));
    cards
}
