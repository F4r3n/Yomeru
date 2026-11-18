//! The Grammar tab of the Review page: the grammar points added from the
//! Lookup page, two cards each. Recognition shows the pattern and asks its
//! meaning; production shows an example's English and the meaning line, and
//! the user writes the Japanese (the phase-4 doc). A session takes either
//! kind, or both mixed ([`GrammarMode`]). Scheduling is the same FSRS path
//! words use.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::dict::{grammar_by_seq, grammar_check, sentence_words};
use crate::drill_log;
use crate::grammar::{
    AnswerCheck, Diff, Example, GrammarEntry, Register, Span, Verdict, WordHint, closest_also,
    hint_words, plain, register, sentence_diff, topic_is_optional,
};
use crate::platform::SentenceWord;
use crate::icons::{Icon, IconName};
use crate::idb::{
    GrammarMode, count_due_grammar, ensure_grammar_production_cards, get_all_cards,
    get_due_grammar_cards, put_card_logged,
};
use crate::settings::load as load_settings;
use crate::srs::{ReviewOutcome, apply_review, now_ms, rating_from_u8};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardDirection, ReviewKind, SrsCard};

use super::lookup_grammar::{ExampleList, GrammarExplanation};
use super::review::next_due_message;
use super::review_common::{
    Answered, RateButtons, ShowAnswerButton, UndoButton, rating_label, undo_answer,
    use_review_shortcuts,
};

/// Examples the back of a recognition card shows.
const BACK_EXAMPLES: usize = 2;

/// The session mode last picked, kept on this device.
const MODE_KEY: &str = "yomeru.grammar_mode";
/// "Answer in my head": production cards without typing, kept on this
/// device (typing suits a keyboard, the head a phone).
const IN_HEAD_KEY: &str = "yomeru.grammar_in_head";

/// One answered grammar card, kept for the end-of-session recap.
#[derive(Clone, PartialEq)]
struct GrammarResult {
    id: String,
    pattern: String,
    meaning: String,
    rating: u8,
}

/// Where the production card on screen stands. Reset for each card.
#[derive(Clone, Default, PartialEq)]
struct Attempt {
    answer: String,
    /// The last check's outcome, for the banner.
    verdict: Option<Verdict>,
    /// A check is on its way.
    checking: bool,
    /// A hint was shown: the formation or the words.
    hinted: bool,
    /// The formation hint is showing.
    formation: bool,
    /// The words hint: `Some` once loaded.
    words: Option<Vec<WordHint>>,
    /// The words hint is on its way.
    words_loading: bool,
    /// The check found the point (or the user says the answer was right).
    accepted: bool,
}

impl Attempt {
    /// The highest rating the card can take: any hint caps it at Hard, the
    /// reference shown before a correct answer at Again.
    fn max_rating(&self) -> u8 {
        match (self.accepted, self.hinted) {
            (true, false) => 4,
            (true, true) => 2,
            (false, _) => 1,
        }
    }
}

/// Whether the production card of `point` is answered by typing: not in
/// "answer in my head", and not for a point the check can't find.
fn typed(point: &GrammarEntry, in_head: bool) -> bool {
    !in_head && point.check != AnswerCheck::SelfGraded
}

fn load_mode() -> GrammarMode {
    crate::rt::kv_get::<String>(MODE_KEY)
        .as_deref()
        .and_then(GrammarMode::parse)
        .unwrap_or(GrammarMode::Mix)
}

/// The example a production card prompts with, picked anew each session so
/// the sentence can't be learned by heart.
fn pick_example(count: usize, seed: u64, position: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let mixed = seed
        .wrapping_add(position as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (mixed >> 32) as usize % count
}

#[component]
pub(crate) fn GrammarReview() -> Element {
    let mut mode = use_signal(load_mode);
    let mut in_head = use_signal(|| crate::rt::kv_get::<bool>(IN_HEAD_KEY).unwrap_or(false));
    let mut due = use_signal(Vec::<SrsCard>::new);
    // The example each production card of `due` prompts with, by position.
    let mut prompts = use_signal(Vec::<usize>::new);
    // Cards due in each mode, in `GrammarMode::ALL` order.
    let mut counts = use_signal(|| [0usize; 3]);
    // Card sequence → its point. Cards whose point left the dictionary are
    // kept out of `due` (their schedule is kept).
    let mut points = use_signal(HashMap::<u32, GrammarEntry>::new);
    let mut idx = use_signal(|| 0usize);
    let mut show_back = use_signal(|| false);
    let mut attempt = use_signal(Attempt::default);
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

    let next_due_of = move |all: &mut Vec<SrsCard>| {
        let m = *mode.peek();
        all.retain(|c| m.takes(c));
        next_due_message(all, now_ms())
    };

    let load_session = move || {
        spawn(async move {
            let settings = load_settings();
            if let Err(e) = ensure_grammar_production_cards().await {
                warn!("ensure_grammar_production_cards failed: {e}");
            }
            let m = *mode.peek();
            let cards = get_due_grammar_cards(now_ms(), m)
                .await
                .unwrap_or_default();
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
            // A production card needs an example to prompt with.
            let cards: Vec<SrsCard> = cards
                .into_iter()
                .filter(|c| {
                    c.grammar_seq()
                        .and_then(|s| map.get(&s))
                        .is_some_and(|p| {
                            c.direction != CardDirection::Production || !p.examples.is_empty()
                        })
                })
                .take(settings.max_session_cards as usize)
                .collect();
            let seed = now_ms() as u64;
            let picks: Vec<usize> = cards
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let n = c
                        .grammar_seq()
                        .and_then(|s| map.get(&s))
                        .map_or(0, |p| p.examples.len());
                    pick_example(n, seed, i)
                })
                .collect();
            points.set(map);
            due.set(cards);
            prompts.set(picks);
            idx.set(0);
            show_back.set(false);
            attempt.set(Attempt::default());
            started.set(false);
            results.set(vec![]);
            last_answer.set(None);
            counts.set(count_due_grammar(now_ms()).await.unwrap_or_default());
            let mut all = get_all_cards().await.unwrap_or_default();
            all.retain(SrsCard::is_grammar);
            has_deck.set(!all.is_empty());
            next_due.set(next_due_of(&mut all));
            loaded.set(true);
        });
    };

    // Reload on mount and whenever a sync lands, but not mid-session.
    use_reload_on_sync(move || {
        if !*started.peek() {
            load_session();
        }
    });

    let mut pick_mode = move |m: GrammarMode| {
        if *mode.peek() == m {
            return;
        }
        mode.set(m);
        let _ = crate::rt::kv_set(MODE_KEY, &m.as_str().to_string());
        load_session();
    };

    // Shows the back. On a production card, before the point was found,
    // that is the reference shown: the rating is capped at Again.
    let mut reveal = move || show_back.set(true);

    let mut check = move || {
        let a = attempt.peek().clone();
        if a.checking || a.answer.trim().is_empty() || *show_back.peek() {
            return;
        }
        let Some(point) = due
            .peek()
            .get(*idx.peek())
            .and_then(SrsCard::grammar_seq)
            .and_then(|s| points.peek().get(&s).cloned())
        else {
            return;
        };
        attempt.with_mut(|a| a.checking = true);
        let card_at = *idx.peek();
        spawn(async move {
            let verdict = grammar_check(&point.id, &a.answer).await;
            // The user moved on (or undid) while the check ran.
            if *idx.peek() != card_at || *show_back.peek() {
                return;
            }
            let verdict = match verdict {
                Ok(v) => v,
                Err(e) => {
                    warn!("grammar_check({}) failed: {e}", point.id);
                    // No check to go by: the user grades.
                    Verdict::SelfGraded
                }
            };
            let found = matches!(verdict, Verdict::Accepted | Verdict::SelfGraded);
            attempt.with_mut(|a| {
                a.checking = false;
                a.accepted = found;
                a.verdict = Some(verdict);
            });
            if found {
                show_back.set(true);
            }
        });
    };

    // Loads the words hint of the card on screen.
    let mut show_words = move || {
        let a = attempt.peek().clone();
        if a.words.is_some() || a.words_loading || *show_back.peek() {
            return;
        }
        let i = *idx.peek();
        let example = due
            .peek()
            .get(i)
            .and_then(SrsCard::grammar_seq)
            .and_then(|s| points.peek().get(&s).cloned())
            .and_then(|p| {
                let k = *prompts.peek().get(i)?;
                p.examples.get(k).cloned()
            });
        let Some(example) = example else {
            return;
        };
        attempt.with_mut(|a| {
            a.hinted = true;
            a.words_loading = true;
        });
        spawn(async move {
            let words = sentence_words(&plain(&example.ja)).await;
            if *idx.peek() != i {
                return;
            }
            let hints = match words {
                Ok(w) => hint_words(&w, &example),
                Err(e) => {
                    warn!("sentence_words failed: {e}");
                    Vec::new()
                }
            };
            attempt.with_mut(|a| {
                a.words_loading = false;
                a.words = Some(hints);
            });
        });
    };

    // The rating cap of the card on screen.
    let max_rating = move || -> u8 {
        let cards = due.peek();
        let Some(card) = cards.get(*idx.peek()) else {
            return 4;
        };
        if card.direction != CardDirection::Production {
            return 4;
        }
        let point = card
            .grammar_seq()
            .and_then(|s| points.peek().get(&s).cloned());
        match point {
            Some(p) if typed(&p, *in_head.peek()) => attempt.peek().max_rating(),
            _ => 4,
        }
    };

    let rate = move |r: u8| {
        if *rating_in_flight.peek() || !*show_back.peek() || r > max_rating() {
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
            attempt.set(Attempt::default());
            if *idx.peek() >= due.peek().len() {
                let mut all = get_all_cards().await.unwrap_or_default();
                next_due.set(next_due_of(&mut all));
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
                    attempt.set(Attempt::default());
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
        let current = *mode.read();
        let counts_v = *counts.read();
        let any_due = counts_v.iter().any(|&n| n > 0);
        let label = if total == 1 {
            "1 grammar card ready".to_string()
        } else {
            format!("{total} grammar cards ready")
        };
        let helper = match current {
            GrammarMode::Mix => "Both kinds of card, one per point.",
            GrammarMode::Recognition => "See the pattern, recall what it means.",
            GrammarMode::Production => "Read the English, write the Japanese.",
        };
        return rsx! {
            if any_due {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Play, size: 44 } }
                    div { class: "headline",
                        if total > 0 { "{label}" } else { "Nothing due in {current.label()}" }
                    }
                    div { class: "seg-tabs grammar-modes",
                        for (m, n) in GrammarMode::ALL.into_iter().zip(counts_v) {
                            button {
                                key: "{m.as_str()}",
                                class: if m == current { "active" } else { "" },
                                "aria-pressed": m == current,
                                disabled: n == 0 && m != current,
                                onclick: move |_| pick_mode(m),
                                "{m.label()} · {n}"
                            }
                        }
                    }
                    div { class: "helper", "{helper}" }
                    if current != GrammarMode::Recognition {
                        label { class: "helper grammar-in-head",
                            input {
                                r#type: "checkbox",
                                checked: *in_head.read(),
                                onchange: move |e| {
                                    let on = e.checked();
                                    in_head.set(on);
                                    let _ = crate::rt::kv_set(IN_HEAD_KEY, &on);
                                },
                            }
                            "Answer in my head (no typing, on this device)"
                        }
                    }
                    button { class: "primary", disabled: total == 0,
                        onclick: move |_| started.set(true),
                        "Start Review"
                    }
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

    let card = cards.get(i).cloned();
    let point = card
        .as_ref()
        .and_then(SrsCard::grammar_seq)
        .and_then(|s| points.read().get(&s).cloned());
    let (Some(card), Some(point)) = (card, point) else {
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
    let production = card.direction == CardDirection::Production;
    let full_open = full_for.read().as_deref() == Some(card.id.as_str());
    let card_id = card.id.clone();

    let header = rsx! {
        div { class: "row", style: "justify-content: space-between;",
            span { class: "muted",
                "Card {i + 1} of {total}"
                if production { " · Production" }
            }
            span { class: "row",
                if can_undo {
                    UndoButton { disabled: in_flight, on_undo: move |_| undo() }
                }
                if show_back_v {
                    span { class: "freq-badge", "{point.jlpt}" }
                }
            }
        }
    };

    if production {
        let example = prompts
            .read()
            .get(i)
            .and_then(|&k| point.examples.get(k).cloned())
            .unwrap_or_default();
        let typing = typed(&point, *in_head.read());
        let a = attempt.read().clone();
        let cap = if typing { a.max_rating() } else { 4 };
        return rsx! {
            div { class: "review-card",
                {header}
                div { class: "face grammar-prompt",
                    div { class: "grammar-prompt-en", "{example.en}" }
                    div { class: "muted grammar-prompt-meaning", "{point.meaning}" }
                }
                if a.formation || show_back_v {
                    ul { class: "grammar-patterns", lang: "ja",
                        for (k, line) in point.formation.iter().enumerate() {
                            li { key: "{k}", "{line}" }
                        }
                    }
                }
                if let Some(words) = a.words.clone() {
                    WordHints { words }
                }
                if !show_back_v {
                    if typing {
                        ProductionInput {
                            attempt,
                            on_check: move |_| check(),
                            on_hint: move |_| attempt.with_mut(|a| {
                                a.hinted = true;
                                a.formation = true;
                            }),
                            on_words: move |_| show_words(),
                            on_reveal: move |_| reveal(),
                        }
                    } else {
                        div { style: "margin-top: 16px;",
                            ShowAnswerButton { on_reveal: move |_| reveal() }
                        }
                    }
                } else {
                    ProductionBack {
                        point: point.clone(),
                        example,
                        answer: if typing { a.answer.clone() } else { String::new() },
                    }
                    if typing && !a.accepted && !a.answer.trim().is_empty() {
                        // The check can miss a valid sentence: the user is
                        // the judge, and only cheats themselves.
                        button { class: "link",
                            onclick: move |_| attempt.with_mut(|a| a.accepted = true),
                            "My answer was right"
                        }
                    }
                    if cap < 4 {
                        div { class: "muted grammar-cap",
                            if cap == 1 {
                                "The answer was shown: Again."
                            } else {
                                "With a hint: Again or Hard."
                            }
                        }
                    }
                    div { style: "margin-top: 16px;",
                        RateButtons { disabled: in_flight, max: cap, on_rate: rate }
                    }
                }
            }
        };
    }

    rsx! {
        div { class: "review-card",
            {header}
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

/// The front of a typed production card: the answer field, the check's
/// banner, the hint and the give-up.
#[component]
fn ProductionInput(
    attempt: Signal<Attempt>,
    on_check: EventHandler<()>,
    on_hint: EventHandler<()>,
    on_words: EventHandler<()>,
    on_reveal: EventHandler<()>,
) -> Element {
    let a = attempt.read().clone();
    let banner = a.verdict.as_ref().and_then(|v| match v {
        Verdict::Close { related } => Some((
            "close",
            format!(
                "{} works here too, but this card practises another point. Try again.",
                related.pattern
            ),
        )),
        Verdict::Other { found } => {
            let used: Vec<&str> = found.iter().map(|r| r.pattern.as_str()).collect();
            Some((
                "miss",
                format!(
                    "You used {}. This card asks for another point.",
                    used.join(", ")
                ),
            ))
        }
        Verdict::NotFound => Some(("miss", "The card's point isn't in your answer yet.".into())),
        Verdict::Accepted | Verdict::SelfGraded => None,
    });
    rsx! {
        div { class: "grammar-answer",
            input {
                r#type: "text",
                lang: "ja",
                class: "grammar-answer-input",
                placeholder: "Write it in Japanese",
                autocomplete: "off",
                autofocus: true,
                "aria-label": "Your answer in Japanese",
                value: "{a.answer}",
                oninput: move |e| {
                    let text = e.value();
                    attempt.with_mut(|a| a.answer = text);
                },
                onkeydown: move |e| {
                    // Enter also ends an IME composition: that one isn't a
                    // submit.
                    if e.key() == Key::Enter && !e.is_composing() {
                        e.prevent_default();
                        on_check.call(());
                    }
                },
            }
            if let Some((class, text)) = banner {
                div { class: "grammar-verdict {class}", role: "status", "{text}" }
            }
            div { class: "row grammar-answer-actions",
                if !a.formation {
                    button { class: "outline", onclick: move |_| on_hint.call(()), "Formation" }
                }
                if a.words.is_none() {
                    button { class: "outline", disabled: a.words_loading,
                        onclick: move |_| on_words.call(()),
                        if a.words_loading { "Words…" } else { "Words" }
                    }
                }
                button { class: "link", onclick: move |_| on_reveal.call(()), "Show answer" }
                button { class: "primary",
                    disabled: a.checking || a.answer.trim().is_empty(),
                    onclick: move |_| on_check.call(()),
                    if a.checking { "Checking…" } else { "Check" }
                    kbd { "aria-hidden": "true", "Enter" }
                }
            }
        }
    }
}

/// The words hint: each word's dictionary form, reading and meaning.
#[component]
fn WordHints(words: Vec<WordHint>) -> Element {
    rsx! {
        if words.is_empty() {
            div { class: "muted grammar-words-none", "No words to hint here." }
        } else {
            ul { class: "grammar-words",
                for (k, w) in words.into_iter().enumerate() {
                    li { key: "{k}",
                        span { class: "w", lang: "ja", "{w.word}" }
                        if !w.reading.is_empty() {
                            span { class: "r", lang: "ja", "{w.reading}" }
                        }
                        span { class: "g", "{w.gloss}" }
                    }
                }
            }
        }
    }
}

/// The back of a production card: the user's answer and the reference,
/// with what one has and the other lacks marked ([`sentence_diff`]), the
/// point marked in the reference, and the point.
#[component]
fn ProductionBack(point: GrammarEntry, example: Example, answer: String) -> Element {
    let reference = plain(&example.ja);
    let typed = !answer.trim().is_empty();
    // The words of the answer and of each sentence it is compared with, so
    // a word is compared by its reading (持って来て, 持ってきて). All or
    // none: until they come, or if one fails, chars are compared as typed.
    let (typed_answer, shown) = (answer.clone(), example.clone());
    let words = use_resource(use_reactive!(|(typed_answer, shown)| async move {
        let (answer, example) = (typed_answer, shown);
        if answer.trim().is_empty() {
            return None;
        }
        let sentences = std::iter::once(answer)
            .chain(std::iter::once(plain(&example.ja)))
            .chain(example.also);
        let mut out: HashMap<String, Vec<SentenceWord>> = HashMap::new();
        for s in sentences {
            let found = sentence_words(&s).await.ok()?;
            out.insert(s, found);
        }
        Some(out)
    }));
    let words = words.read().clone().flatten();
    let optional_topic = topic_is_optional(&example);
    let diff = |a: &str, b: &str| -> Diff {
        let found = |s: &str| words.as_ref().and_then(|w| w.get(s)).map(Vec::as_slice);
        sentence_diff(a, found(a), b, found(b), optional_topic)
    };
    // Another correct answer of the example, when the answer is closer to
    // it: the marks are then against it, and the reference is shown plain.
    let also = if typed { closest_also(&answer, &example, diff).map(str::to_owned) } else { None };
    let compared = also.as_deref().unwrap_or(&reference);
    let d = if typed { diff(&answer, compared) } else { Diff::default() };
    // The point's chars in the reference.
    let in_point: Vec<bool> = example
        .ja
        .iter()
        .flat_map(|span| {
            let on = matches!(span, Span::Highlight { .. });
            std::iter::repeat_n(on, span.text().chars().count())
        })
        .collect();
    // The English rarely says how polite to be: only a note, never a miss.
    let register_note = if !typed {
        None
    } else {
        match (register(&answer), register(compared)) {
            (Register::Polite, Register::Plain) => Some(
                "The reference is casual (plain forms, だ); you wrote it politely (です/ます). Both are right.",
            ),
            (Register::Plain, Register::Polite) => Some(
                "The reference is polite (です/ます); you wrote it casually (plain forms, だ). Both are right.",
            ),
            _ => None,
        }
    };
    // The class of a char: marked, left out of the comparison, or neither.
    let class = |flags: &[bool], optional: &Option<std::ops::Range<usize>>, k: usize, marked: &'static str| {
        if optional.as_ref().is_some_and(|r| r.contains(&k)) {
            "diff-optional"
        } else if flags.get(k).copied().unwrap_or(false) {
            marked
        } else {
            ""
        }
    };
    let answer_runs = runs(&answer, |k| class(&d.a, &d.a_optional, k, "diff-extra"));
    let also_runs = also.as_deref().map(|a| runs(a, |k| class(&d.b, &d.b_optional, k, "diff-missing")));
    let reference_runs = runs(&reference, |k| {
        (
            if also.is_none() { class(&d.b, &d.b_optional, k, "diff-missing") } else { "" },
            in_point.get(k).copied().unwrap_or(false),
        )
    });
    rsx! {
        div { class: "grammar-back",
            if typed {
                div { class: "grammar-your-answer",
                    div { class: "muted", "Your answer" }
                    div { class: "jp", lang: "ja",
                        for (k, (text, class)) in answer_runs.into_iter().enumerate() {
                            span { key: "{k}", class, "{text}" }
                        }
                    }
                }
            }
            if let Some(also_runs) = also_runs {
                div { class: "grammar-your-answer",
                    div { class: "muted", "Also correct" }
                    div { class: "jp", lang: "ja",
                        for (k, (text, class)) in also_runs.into_iter().enumerate() {
                            span { key: "{k}", class, "{text}" }
                        }
                    }
                }
            }
            div { class: "grammar-examples",
                div { class: "grammar-example",
                    if typed {
                        div { class: "muted", "Reference" }
                    }
                    div { class: "jp", lang: "ja",
                        for (k, (text, (class, point_part))) in reference_runs.into_iter().enumerate() {
                            if point_part {
                                mark { key: "{k}", class, "{text}" }
                            } else {
                                span { key: "{k}", class, "{text}" }
                            }
                        }
                    }
                    div { class: "en", "{example.en}" }
                }
            }
            if let Some(note) = register_note {
                div { class: "grammar-verdict register", "{note}" }
            }
            div { class: "grammar-back-point",
                span { class: "w", lang: "ja", "{point.pattern}" }
                span { class: "muted", " · {point.meaning}" }
            }
        }
    }
}

/// `text` cut into runs of chars sharing `style(char index)`.
fn runs<S: PartialEq + Copy>(text: &str, style: impl Fn(usize) -> S) -> Vec<(String, S)> {
    let mut out: Vec<(String, S)> = Vec::new();
    for (k, c) in text.chars().enumerate() {
        let st = style(k);
        match out.last_mut() {
            Some((run, last)) if *last == st => run.push(c),
            _ => out.push((c.to_string(), st)),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_cap_the_rating() {
        let a = |accepted, hinted| Attempt {
            accepted,
            hinted,
            ..Attempt::default()
        };
        assert_eq!(a(true, false).max_rating(), 4);
        assert_eq!(a(true, true).max_rating(), 2);
        assert_eq!(a(false, false).max_rating(), 1);
        assert_eq!(a(false, true).max_rating(), 1);
    }

    #[test]
    fn the_example_is_one_of_the_points() {
        for seed in [0, 1, 1_700_000_000_000, u64::MAX] {
            for pos in 0..20 {
                assert!(pick_example(5, seed, pos) < 5);
            }
        }
        assert_eq!(pick_example(0, 7, 3), 0);
        // Not always the first.
        assert!((0..20).any(|p| pick_example(5, 1_700_000_000_000, p) != 0));
    }
}
