//! The Grammar tab of the Review page: one card per grammar point added from
//! the Lookup page, shown pattern → meaning, formation and examples (the
//! recognition face of the phase-4 doc). Scheduling is the same FSRS path
//! words use.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::dict::grammar_by_seq;
use crate::drill_log;
use crate::grammar::GrammarEntry;
use crate::icons::{Icon, IconName};
use crate::idb::{get_all_cards, get_due_grammar_cards, put_card_logged};
use crate::settings::load as load_settings;
use crate::srs::{ReviewOutcome, apply_review, now_ms, rating_from_u8};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{ReviewKind, SrsCard};

use super::lookup_grammar::{ExampleList, GrammarExplanation};
use super::review::next_due_message;
use super::review_common::{
    Answered, RateButtons, ShowAnswerButton, UndoButton, rating_label, undo_answer,
    use_review_shortcuts,
};

/// Examples the back of a card shows.
const BACK_EXAMPLES: usize = 2;

/// One answered grammar card, kept for the end-of-session recap.
#[derive(Clone, PartialEq)]
struct GrammarResult {
    id: String,
    pattern: String,
    meaning: String,
    rating: u8,
}

#[component]
pub(crate) fn GrammarReview() -> Element {
    let mut due = use_signal(Vec::<SrsCard>::new);
    // Card sequence → its point. Cards whose point left the dictionary are
    // kept out of `due` (their schedule is kept).
    let mut points = use_signal(HashMap::<u32, GrammarEntry>::new);
    let mut idx = use_signal(|| 0usize);
    let mut show_back = use_signal(|| false);
    let mut started = use_signal(|| false);
    let mut loaded = use_signal(|| false);
    let mut has_deck = use_signal(|| false);
    let mut next_due = use_signal(|| Option::<String>::None);
    let mut results = use_signal(Vec::<GrammarResult>::new);
    // The card only advances once its write lands, so a second press in that
    // window must not rate it again.
    let mut rating_in_flight = use_signal(|| false);
    // The last answer, while it can still be taken back.
    let mut last_answer = use_signal(|| Option::<Answered>::None);
    // The card whose full explanation is open on the back. Keyed by id, so
    // it closes by itself once the session moves on (or undo steps back).
    let mut full_for = use_signal(|| Option::<String>::None);

    let load_session = move || {
        spawn(async move {
            let settings = load_settings();
            let cards = get_due_grammar_cards(now_ms()).await.unwrap_or_default();
            let seqs: Vec<u32> = cards.iter().filter_map(SrsCard::grammar_seq).collect();
            let found = match grammar_by_seq(&seqs).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("grammar_by_seq({} due cards) failed: {e}", seqs.len());
                    Vec::new()
                }
            };
            let map: HashMap<u32, GrammarEntry> = seqs
                .into_iter()
                .zip(found)
                .filter_map(|(s, e)| Some((s, e?)))
                .collect();
            let cards: Vec<SrsCard> = cards
                .into_iter()
                .filter(|c| c.grammar_seq().is_some_and(|s| map.contains_key(&s)))
                .take(settings.max_session_cards as usize)
                .collect();
            points.set(map);
            due.set(cards);
            idx.set(0);
            show_back.set(false);
            started.set(false);
            results.set(vec![]);
            last_answer.set(None);
            let mut all = get_all_cards().await.unwrap_or_default();
            all.retain(SrsCard::is_grammar);
            has_deck.set(!all.is_empty());
            next_due.set(next_due_message(&all, now_ms()));
            loaded.set(true);
        });
    };

    // Reload on mount and whenever a sync lands, but not mid-session.
    use_reload_on_sync(move || {
        if !*started.peek() {
            load_session();
        }
    });

    let mut reveal = move || show_back.set(true);

    let rate = move |r: u8| {
        if *rating_in_flight.peek() || !*show_back.peek() {
            return;
        }
        let Some(card) = due.peek().get(*idx.peek()).cloned() else {
            return;
        };
        let Some(point) = card.grammar_seq().and_then(|s| points.peek().get(&s).cloned()) else {
            return;
        };
        let i = *idx.peek();
        let results_before = results.peek().len();
        results.with_mut(|v| {
            v.push(GrammarResult {
                id: card.id.clone(),
                pattern: point.pattern.clone(),
                meaning: point.meaning.clone(),
                rating: r,
            })
        });
        rating_in_flight.set(true);
        spawn(async move {
            let settings = load_settings();
            let (ReviewOutcome::Rescheduled(c) | ReviewOutcome::Graduated(c)) =
                apply_review(&card, rating_from_u8(r), now_ms(), &settings);
            let row = drill_log::answer(&card, r, ReviewKind::Review);
            match put_card_logged(&c, &row).await {
                Ok(()) => {
                    last_answer.set(Some(Answered {
                        prev: Some(card.clone()),
                        row,
                        idx: i,
                        results: results_before,
                    }));
                    schedule_sync();
                }
                Err(e) => warn!("put_card({}) after grammar review failed: {e}", card.id),
            }
            idx += 1;
            show_back.set(false);
            if *idx.peek() >= due.peek().len() {
                let mut all = get_all_cards().await.unwrap_or_default();
                all.retain(SrsCard::is_grammar);
                next_due.set(next_due_message(&all, now_ms()));
            }
            // Last, so an undo can't run before the summary lands.
            rating_in_flight.set(false);
        });
    };

    // Steps back to the last answered card, question side up.
    let mut undo = move || {
        if *rating_in_flight.peek() || !*started.peek() {
            return;
        }
        let Some(last) = last_answer.peek().clone() else {
            return;
        };
        rating_in_flight.set(true);
        spawn(async move {
            match undo_answer(&last).await {
                Ok(()) => {
                    last_answer.set(None);
                    results.with_mut(|v| v.truncate(last.results));
                    idx.set(last.idx);
                    show_back.set(false);
                    next_due.set(None);
                }
                Err(e) => warn!("undo of {} failed: {e}", last.row.card_id),
            }
            rating_in_flight.set(false);
        });
    };

    use_review_shortcuts(
        move || *started.peek() && *idx.peek() < due.peek().len(),
        show_back,
        reveal,
        rate,
        undo,
    );
    let can_undo = *started.read() && last_answer.read().is_some();

    if !*loaded.read() {
        return rsx! { div { class: "loading", "Loading grammar…" } };
    }

    let cards = due.read();
    let i = *idx.read();
    let total = cards.len();

    if !*started.read() {
        let label = if total == 1 {
            "1 grammar point ready".to_string()
        } else {
            format!("{total} grammar points ready")
        };
        return rsx! {
            if total > 0 {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Play, size: 44 } }
                    div { class: "headline", "{label}" }
                    div { class: "helper", "Grammar points you added from the Lookup tab." }
                    button { class: "primary", onclick: move |_| started.set(true), "Start Review" }
                }
            } else if !*has_deck.read() {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Sparkles, size: 44 } }
                    div { class: "headline", "No grammar yet" }
                    div { class: "helper",
                        "Add grammar points from the Lookup tab, then accept them in New Words."
                    }
                }
            } else {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                    div { class: "headline", "All caught up" }
                    div { class: "helper",
                        if let Some(m) = next_due.read().clone() { "{m}" } else { "No grammar due right now." }
                    }
                }
            }
        };
    }

    let point = cards
        .get(i)
        .and_then(SrsCard::grammar_seq)
        .and_then(|s| points.read().get(&s).cloned());
    let Some(point) = point else {
        let (forgot, remembered): (Vec<GrammarResult>, Vec<GrammarResult>) =
            results.read().iter().cloned().partition(|r| r.rating == 1);
        return rsx! {
            div { class: "empty-state",
                div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                div { class: "headline", "Review complete!" }
                div { class: "helper",
                    "{remembered.len()} remembered · {forgot.len()} forgot"
                    if let Some(m) = next_due.read().clone() { " · {m}" }
                }
            }
            if !forgot.is_empty() || !remembered.is_empty() {
                div { class: "recap-grid",
                    GrammarRecapList {
                        title: format!("Forgot · {}", forgot.len()),
                        title_class: "section-title error",
                        empty: "Nothing forgotten — clean session.",
                        results: forgot,
                    }
                    GrammarRecapList {
                        title: format!("Remembered · {}", remembered.len()),
                        title_class: "section-title ok",
                        empty: "Nothing remembered this time.",
                        results: remembered,
                    }
                }
            }
            div { class: "row", style: "justify-content: center; gap: 8px; margin-top: 16px;",
                if can_undo {
                    UndoButton { disabled: *rating_in_flight.read(), on_undo: move |_| undo() }
                }
                button { class: "primary", onclick: move |_| load_session(), "Done" }
            }
        };
    };

    let show_back_v = *show_back.read();
    let in_flight = *rating_in_flight.read();
    let card_id = cards.get(i).map(|c| c.id.clone()).unwrap_or_default();
    let full_open = full_for.read().as_deref() == Some(card_id.as_str());

    rsx! {
        div { class: "review-card",
            div { class: "row", style: "justify-content: space-between;",
                span { class: "muted", "Card {i + 1} of {total}" }
                span { class: "row",
                    if can_undo {
                        UndoButton { disabled: in_flight, on_undo: move |_| undo() }
                    }
                    if show_back_v {
                        span { class: "freq-badge", "{point.jlpt}" }
                    }
                }
            }
            div { class: "face",
                div { class: "word grammar-face", lang: "ja", "{point.pattern}" }
            }
            if show_back_v {
                div { class: "grammar-back",
                    div { class: "gloss grammar-back-meaning", "{point.meaning}" }
                    if full_open {
                        GrammarExplanation { entry: point.clone() }
                    } else {
                        ul { class: "grammar-patterns", lang: "ja",
                            for (k, line) in point.formation.iter().enumerate() {
                                li { key: "{k}", "{line}" }
                            }
                        }
                        if !point.examples.is_empty() {
                            ExampleList {
                                examples: point.examples.iter().take(BACK_EXAMPLES).cloned().collect::<Vec<_>>(),
                            }
                        }
                    }
                    button {
                        class: "link",
                        onclick: move |_| {
                            let open = full_for.peek().as_deref() == Some(card_id.as_str());
                            full_for.set(if open { None } else { Some(card_id.clone()) });
                        },
                        if full_open { "Hide full explanation ▴" } else { "Full explanation ▾" }
                    }
                }
            }
            div { style: "margin-top: 16px;",
                if !show_back_v {
                    ShowAnswerButton { on_reveal: move |_| reveal() }
                } else {
                    RateButtons { disabled: in_flight, on_rate: rate }
                }
            }
        }
    }
}

/// One column of the end-of-session recap.
#[component]
fn GrammarRecapList(
    title: String,
    title_class: &'static str,
    empty: &'static str,
    results: Vec<GrammarResult>,
) -> Element {
    rsx! {
        div { class: "card",
            div { class: "{title_class}", "{title}" }
            if results.is_empty() {
                div { class: "muted", style: "font-size: 13px;", "{empty}" }
            } else {
                div { class: "word-rows",
                    for r in results {
                        div { key: "{r.id}", class: "word-row",
                            span { class: "w", lang: "ja", "{r.pattern}" }
                            span { class: "g", title: "{r.meaning}", "{r.meaning}" }
                            span {
                                class: if r.rating == 1 { "badge fail" } else { "badge" },
                                "{rating_label(r.rating)}"
                            }
                        }
                    }
                }
            }
        }
    }
}
