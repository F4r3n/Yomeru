use dioxus::prelude::*;

use crate::idb::{delete_card, get_staging_cards, promote_card};
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
            let _ = promote_card(&word).await;
            reload();
        });
    };

    let reject_one = move |word: String| {
        spawn(async move {
            let _ = delete_card(&word).await;
            reload();
        });
    };

    let promote_all = move |_| {
        spawn(async move {
            let staging = get_staging_cards().await.unwrap_or_default();
            for w in unique_by_word(staging).into_iter().map(|c| c.word) {
                let _ = promote_card(&w).await;
            }
            reload();
        });
    };

    rsx! {
        div { class: "col",
            if *loading.read() {
                div { class: "loading", "Loading…" }
            } else if let Some(e) = err.read().clone() {
                div { class: "card error", "Failed: {e}" }
            } else if cards.read().is_empty() {
                div { class: "empty", "No staged words. Add some from the Lookup tab." }
            } else {
                div { class: "row", style: "justify-content: space-between;",
                    span { class: "muted", "{cards.read().len()} staged" }
                    button { class: "primary", onclick: promote_all, "Promote all" }
                }
                for card in cards.read().iter().cloned() {
                    {
                        let word = card.word.clone();
                        let word_a = word.clone();
                        let word_b = word.clone();
                        rsx! {
                            div { class: "card row", style: "justify-content: space-between;",
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

fn unique_by_word(mut cards: Vec<SrsCard>) -> Vec<SrsCard> {
    let mut seen = std::collections::HashSet::new();
    cards.retain(|c| seen.insert(c.word.clone()));
    cards
}
