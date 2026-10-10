//! The Kanji tab of the Review page: one card per kanji from the words under
//! review, shown kanji → meanings + readings. The deck is built by
//! [`crate::kanji_deck`]; scheduling is the same FSRS path words use.

use std::collections::HashMap;

use dioxus::prelude::*;
use kanjidic_types::KanjiEntry;

use crate::dict::kanji_for;
use crate::icons::{Icon, IconName};
use crate::idb::{get_all_cards, get_due_kanji_cards, put_card};
use crate::kanji_deck::{StudiedWord, ensure_kanji_cards};
use crate::settings::load as load_settings;
use crate::srs::{ReviewOutcome, apply_review, now_ms, rating_from_u8};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::SrsCard;

use super::review::next_due_message;
use super::review_common::{RateButtons, ShowAnswerButton, use_review_shortcuts};

/// Whether there are any kanji cards at all, and "Next card due in …" across
/// them if anything is scheduled.
async fn kanji_deck_state() -> (bool, Option<String>) {
    let mut all = get_all_cards().await.unwrap_or_default();
    all.retain(SrsCard::is_kanji);
    (!all.is_empty(), next_due_message(&all, now_ms()))
}

#[component]
pub(crate) fn KanjiReview() -> Element {
    let mut due = use_signal(Vec::<SrsCard>::new);
    let mut idx = use_signal(|| 0usize);
    let mut show_back = use_signal(|| false);
    let mut started = use_signal(|| false);
    let mut loaded = use_signal(|| false);
    let mut load_error = use_signal(|| Option::<String>::None);
    // Kanji → the studied words using it, for the back of the card.
    let mut words = use_signal(HashMap::<char, Vec<StudiedWord>>::new);
    let mut next_due = use_signal(|| Option::<String>::None);
    // Kanji cards outlive their words, so the deck can hold cards even when no
    // current word has kanji.
    let mut has_deck = use_signal(|| false);
    let mut remembered = use_signal(|| 0usize);
    let mut forgot = use_signal(|| 0usize);
    // Same guard as word review: the card only advances once its write lands,
    // so a second press in that window must not rate it again.
    let mut rating_in_flight = use_signal(|| false);

    let load_session = move || {
        spawn(async move {
            match ensure_kanji_cards().await {
                Ok(map) => {
                    words.set(map);
                    load_error.set(None);
                }
                Err(e) => {
                    warn!("ensure_kanji_cards failed: {e}");
                    load_error.set(Some(e));
                }
            }
            let settings = load_settings();
            let cards: Vec<SrsCard> = get_due_kanji_cards(now_ms())
                .await
                .unwrap_or_default()
                .into_iter()
                .take(settings.max_session_cards as usize)
                .collect();
            due.set(cards);
            idx.set(0);
            show_back.set(false);
            started.set(false);
            remembered.set(0);
            forgot.set(0);
            let (any, next) = kanji_deck_state().await;
            has_deck.set(any);
            next_due.set(next);
            loaded.set(true);
        });
    };

    // Reload on mount and whenever a sync lands, but not mid-session.
    use_reload_on_sync(move || {
        if !*started.peek() {
            load_session();
        }
    });

    // Dictionary details for the card on screen, fetched as soon as it shows
    // so the back is ready when revealed.
    let info = use_resource(move || async move {
        let kanji = due.read().get(*idx.read()).and_then(SrsCard::kanji_char)?;
        kanji_for(&kanji.to_string())
            .await
            .ok()?
            .into_iter()
            .find(|k| k.literal == kanji)
    });

    let mut reveal = move || show_back.set(true);

    let rate = move |r: u8| {
        if *rating_in_flight.peek() || !*show_back.peek() {
            return;
        }
        let Some(card) = due.peek().get(*idx.peek()).cloned() else {
            return;
        };
        if r == 1 {
            forgot += 1;
        } else {
            remembered += 1;
        }
        rating_in_flight.set(true);
        spawn(async move {
            let settings = load_settings();
            let (ReviewOutcome::Rescheduled(c) | ReviewOutcome::Graduated(c)) =
                apply_review(&card, rating_from_u8(r), now_ms(), &settings);
            match put_card(&c).await {
                Ok(()) => schedule_sync(),
                Err(e) => warn!("put_card({}) after kanji review failed: {e}", card.id),
            }
            idx += 1;
            show_back.set(false);
            rating_in_flight.set(false);
            if *idx.peek() >= due.peek().len() {
                next_due.set(kanji_deck_state().await.1);
            }
        });
    };

    use_review_shortcuts(
        move || *started.peek() && *idx.peek() < due.peek().len(),
        show_back,
        reveal,
        rate,
    );

    if !*loaded.read() {
        return rsx! { div { class: "loading", "Building kanji deck…" } };
    }

    let cards = due.read();
    let i = *idx.read();
    let total = cards.len();

    if !*started.read() {
        let label = if total == 1 {
            "1 kanji ready".to_string()
        } else {
            format!("{total} kanji ready")
        };
        let deck_empty = !*has_deck.read();
        return rsx! {
            if let Some(e) = load_error.read().clone() {
                div { class: "card error", "Couldn't update the kanji deck: {e}" }
            }
            if total > 0 {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Play, size: 44 } }
                    div { class: "headline", "{label}" }
                    div { class: "helper", "Kanji from the words you are reviewing." }
                    button { class: "primary", onclick: move |_| started.set(true), "Start Review" }
                }
            } else if deck_empty {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Sparkles, size: 44 } }
                    div { class: "headline", "No kanji yet" }
                    div { class: "helper",
                        "Kanji cards appear here once you review words written with kanji."
                    }
                }
            } else {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                    div { class: "headline", "All caught up" }
                    div { class: "helper",
                        if let Some(m) = next_due.read().clone() { "{m}" } else { "No kanji due right now." }
                    }
                }
            }
        };
    }

    let Some(kanji) = cards.get(i).and_then(SrsCard::kanji_char) else {
        let (ok, ko) = (*remembered.read(), *forgot.read());
        return rsx! {
            div { class: "empty-state",
                div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                div { class: "headline", "Review complete!" }
                div { class: "helper",
                    "{ok} remembered · {ko} forgot"
                    if let Some(m) = next_due.read().clone() { " · {m}" }
                }
                button { class: "primary", onclick: move |_| load_session(), "Done" }
            }
        };
    };

    let show_back_v = *show_back.read();
    let in_flight = *rating_in_flight.read();
    let used_in = words.read().get(&kanji).cloned().unwrap_or_default();
    // The resource keeps its last value until the new lookup lands, so right
    // after a rating it can still hold the previous kanji's entry.
    let entry = info.read().clone().flatten().filter(|k| k.literal == kanji);

    rsx! {
        div { class: "review-card",
            div { class: "row", style: "justify-content: space-between;",
                span { class: "muted", "Card {i + 1} of {total}" }
                span { class: "row",
                    if show_back_v {
                        if let Some(k) = &entry {
                            span { class: "muted kanji-facts", "{kanji_facts(k)}" }
                        }
                    }
                    span { class: "badge", "Kanji" }
                }
            }
            div { class: "face",
                div { class: "word kanji-face", "{kanji}" }
            }
            if show_back_v {
                KanjiBack { info: entry.clone(), used_in }
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

/// Strokes, grade and JLPT level: secondary, so shown small in the card's
/// corner rather than on the back itself.
fn kanji_facts(k: &KanjiEntry) -> String {
    let mut facts = vec![format!("{} strokes", k.stroke_count)];
    if let Some(g) = k.grade {
        facts.push(format!("grade {g}"));
    }
    if let Some(j) = k.jlpt {
        facts.push(format!("JLPT {j}"));
    }
    facts.join(" · ")
}

/// Back of a kanji card: meanings, readings, and the words
/// under review that use it. Each word folds open to its reading and meaning,
/// closed at first so the card stays about the kanji.
#[component]
fn KanjiBack(info: Option<KanjiEntry>, used_in: Vec<StudiedWord>) -> Element {
    let Some(k) = info else {
        return rsx! { div { class: "muted", style: "text-align: center;", "No dictionary entry for this kanji." } };
    };
    rsx! {
        div { class: "kanji-back",
            div { class: "meanings", "{k.meanings.join(\", \")}" }
            if !k.on_readings.is_empty() {
                div { span { class: "muted", "On: " } "{k.on_readings.join(\"、\")}" }
            }
            if !k.kun_readings.is_empty() {
                div { span { class: "muted", "Kun: " } "{k.kun_readings.join(\"、\")}" }
            }
            if !used_in.is_empty() {
                div { class: "used-in",
                    div { class: "muted", "Your words" }
                    for w in used_in {
                        details { key: "{w.sequence}", class: "used-word",
                            summary { "{w.headword}" }
                            div { class: "used-word-info",
                                if !w.reading.is_empty() {
                                    span { class: "reading", "{w.reading}" }
                                }
                                if !w.gloss.is_empty() {
                                    span { "{w.gloss}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
