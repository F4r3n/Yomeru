use dioxus::prelude::*;
use jmdict_types::WordEntry;

use crate::app::Route;
use crate::components::pos_list;
use crate::dict::{
    direction_label, examples_for, kanji_for, lookup_by_sequence, preferred_headword,
    primary_reading, short_gloss,
};
use crate::drill_log;
use crate::icons::{Icon, IconName};
use crate::idb::{get_all_cards, get_due_cards, get_staging_cards, put_card_logged};
use crate::settings::load as load_settings;
use crate::srs::{
    DRILL_BATCH, ReviewOutcome, TROUBLE_EXIT_INTERVAL_DAYS, TROUBLE_MIN_FAILS, TroubleWord,
    apply_review, now_ms, pick_drill_batch, rating_from_u8, trouble_words,
};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardDirection, CardStatus, ReviewKind as LogKind, SrsCard};

use super::review_common::{
    RateButtons, ShowAnswerButton, TabButton, rating_label, use_review_shortcuts,
};
use super::review_kanji::KanjiReview;

#[derive(Clone, Copy, PartialEq)]
enum BackTab {
    Word,
    Kanji,
    Examples,
}

/// One answered card, kept for the end-of-session recap.
#[derive(Clone, PartialEq)]
struct SessionResult {
    id: String,
    sequence: u32,
    direction: CardDirection,
    entry: WordEntry,
    rating: u8,
}

/// First two senses' glosses, flattened — enough to jog the memory in a
/// one-line row without crowding it.
pub(crate) fn shuffle<T>(mut v: Vec<T>) -> Vec<T> {
    // Fisher-Yates with Math.random() — good enough for review-order randomness.
    for i in (1..v.len()).rev() {
        let r = (crate::rt::random() * (i as f64 + 1.0)) as usize;
        v.swap(i, r);
    }
    v
}

async fn attach_entries(cards: Vec<SrsCard>) -> (Vec<SrsCard>, Vec<WordEntry>, Vec<String>) {
    if cards.is_empty() {
        return (vec![], vec![], vec![]);
    }
    let seqs: Vec<u32> = cards.iter().map(|c| c.sequence).collect();
    let hits = lookup_by_sequence(&seqs).await.unwrap_or_default();
    let mut kept = Vec::new();
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    for (c, entry) in cards.into_iter().zip(hits) {
        if entry.is_none() && matches!(c.direction, CardDirection::Recall) {
            // Recall front needs glosses; without them the card is unreviewable.
            skipped.push(format!("(seq {}) recall", c.sequence));
            continue;
        }
        entries.push(entry.unwrap_or_else(|| WordEntry {
            sequence: c.sequence,
            kanji_forms: vec![],
            reading_forms: vec![],
            senses: vec![],
        }));
        kept.push(c);
    }
    (kept, entries, skipped)
}

/// "Next card due in …" for the cards a tab reviews; callers pass only their
/// own kind (words or kanji).
pub(crate) fn next_due_message(cards: &[SrsCard], now: f64) -> Option<String> {
    let next = cards
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Active) && c.due_ms > now)
        .map(|c| c.due_ms)
        .fold(f64::INFINITY, f64::min);
    if next.is_finite() {
        let mins = ((next - now) / 60_000.0).round() as i64;
        Some(if mins < 60 {
            format!("Next card due in {mins} min")
        } else {
            format!("Next card due in {} hr", (mins / 60).max(1))
        })
    } else {
        None
    }
}

/// localStorage key remembering which review tab was open last.
const REVIEW_TAB_KEY: &str = "yomeru.review_tab";

#[derive(Clone, Copy, PartialEq)]
enum ReviewKind {
    Words,
    Kanji,
}

/// The Review page: word and kanji reviews as separate sessions, one tab each.
/// Only the open tab is mounted, so only its keyboard shortcuts are live.
#[component]
pub fn ReviewTab() -> Element {
    let mut kind = use_signal(
        || match crate::rt::kv_get::<String>(REVIEW_TAB_KEY).as_deref() {
            Some("kanji") => ReviewKind::Kanji,
            _ => ReviewKind::Words,
        },
    );
    let mut select = move |k: ReviewKind| {
        kind.set(k);
        let _ = crate::rt::kv_set(
            REVIEW_TAB_KEY,
            &if k == ReviewKind::Kanji {
                "kanji"
            } else {
                "words"
            },
        );
    };
    let current = *kind.read();
    rsx! {
        div {
            div { class: "subtabs", style: "margin-bottom: 14px;",
                TabButton { active: current == ReviewKind::Words, onclick: move |_| select(ReviewKind::Words), label: "Words" }
                TabButton { active: current == ReviewKind::Kanji, onclick: move |_| select(ReviewKind::Kanji), label: "Kanji" }
            }
            match current {
                ReviewKind::Words => rsx! { WordReview {} },
                ReviewKind::Kanji => rsx! { KanjiReview {} },
            }
        }
    }
}

#[component]
fn WordReview() -> Element {
    let mut due_cards = use_signal(Vec::<SrsCard>::new);
    let mut entries = use_signal(Vec::<WordEntry>::new);
    let mut skipped = use_signal(Vec::<String>::new);
    let mut idx = use_signal(|| 0usize);
    let mut show_back = use_signal(|| false);
    let mut started = use_signal(|| false);
    let mut staging_count = use_signal(|| 0usize);
    let mut next_due = use_signal(|| Option::<String>::None);
    let mut graduated_msg = use_signal(|| Option::<String>::None);
    let mut back_tab = use_signal(|| BackTab::Word);
    let mut kanji = use_signal(Vec::<kanjidic_types::KanjiEntry>::new);
    let mut examples = use_signal(Vec::<examples_types::ExampleEntry>::new);
    // The session loads in two stages so the start screen doesn't wait for
    // the dictionary (a cold start on Android loads it in the background):
    // `cards_loaded` once the due cards are read from the store,
    // `entries_ready` once their words are looked up. A session can only
    // start after the second.
    let mut cards_loaded = use_signal(|| false);
    let mut entries_ready = use_signal(|| false);
    // Bumped by every load. A sync can start a new load while an older one
    // is still awaiting; the older one then drops its results.
    let load_gen = use_signal(|| 0u32);
    let mut trouble = use_signal(Vec::<(TroubleWord, Option<WordEntry>)>::new);
    // Drill sessions replay trouble words without touching their schedule:
    // ratings feed the recap and nothing else.
    let mut drill = use_signal(|| false);
    // Drill counts from the answer log, which rotate the batch a drill picks.
    let mut drilled = use_signal(std::collections::HashMap::<u32, u32>::new);
    let mut show_all_trouble = use_signal(|| false);
    let mut results = use_signal(Vec::<SessionResult>::new);
    // Set while a rating is being saved: the card only advances once the write
    // lands, and a second press in that window would rate the same card again
    // and then skip the next one.
    let mut rating_in_flight = use_signal(|| false);

    let load_session = move || {
        let generation = *load_gen.peek() + 1;
        // A copy of the handle, so this closure stays `Fn`.
        let mut gen_signal = load_gen;
        gen_signal.set(generation);
        let current = move || *load_gen.peek() == generation;
        spawn(async move {
            // Stage 1: the cards alone, enough for the start screen.
            let settings = load_settings();
            let now = now_ms();
            let due = get_due_cards(now).await.unwrap_or_default();
            let limited: Vec<SrsCard> = shuffle(
                due.into_iter()
                    .take(settings.max_session_cards as usize)
                    .collect(),
            );
            let staging = get_staging_cards().await.unwrap_or_default();
            if !current() {
                return;
            }
            // Unique by sequence — recognition and recall siblings count once.
            let mut seen = std::collections::HashSet::new();
            let n = staging
                .into_iter()
                .filter(|c| seen.insert(c.sequence))
                .count();
            staging_count.set(n);
            due_cards.set(limited.clone());
            entries.set(vec![]);
            entries_ready.set(false);
            skipped.set(vec![]);
            idx.set(0);
            show_back.set(false);
            started.set(false);
            graduated_msg.set(None);
            next_due.set(None);
            drill.set(false);
            results.set(vec![]);
            cards_loaded.set(true);

            // Stage 2: their words. Waits for the dictionary on a cold start.
            // `attach_entries` keeps the cards' (shuffled) order and drops the
            // ones it can't show, so cards and entries stay aligned.
            let (kept, ents, skips) = attach_entries(limited).await;
            if !current() {
                return;
            }
            due_cards.set(kept);
            entries.set(ents);
            skipped.set(skips);
            entries_ready.set(true);

            // The trouble list under the start screen, which shows words too.
            let all = get_all_cards().await.unwrap_or_default();
            let words = trouble_words(&all, TROUBLE_MIN_FAILS, TROUBLE_EXIT_INTERVAL_DAYS);
            let counts = drill_log::load().await;
            let seqs: Vec<u32> = words.iter().map(|w| w.sequence).collect();
            let hits = if seqs.is_empty() {
                vec![]
            } else {
                lookup_by_sequence(&seqs).await.unwrap_or_default()
            };
            if !current() {
                return;
            }
            drilled.set(counts);
            trouble.set(words.into_iter().zip(hits).collect());
        });
    };

    // Reload on mount and whenever a sync lands, but don't yank a session the
    // user has already started.
    use_reload_on_sync(move || {
        if !*started.peek() {
            load_session();
        }
    });

    let start_review = move |_| {
        // The button is disabled until then; this also covers a queued click.
        if *entries_ready.peek() {
            started.set(true);
        }
    };

    let start_drill = move |_| {
        let words: Vec<TroubleWord> = trouble.read().iter().map(|(w, _)| w.clone()).collect();
        let cards: Vec<SrsCard> = pick_drill_batch(&words, &drilled.read(), DRILL_BATCH)
            .into_iter()
            .flat_map(|w| w.cards.iter().cloned())
            .collect();
        spawn(async move {
            let (kept, ents, skips) = attach_entries(cards).await;
            let paired: Vec<_> = shuffle(kept.into_iter().zip(ents).collect());
            let (kept_final, ents_final) = paired.into_iter().unzip();
            due_cards.set(kept_final);
            entries.set(ents_final);
            skipped.set(skips);
            idx.set(0);
            show_back.set(false);
            graduated_msg.set(None);
            next_due.set(None);
            results.set(vec![]);
            drill.set(true);
            started.set(true);
        });
    };

    let mut reveal_answer = move || {
        if *show_back.peek() {
            return;
        }
        show_back.set(true);
        back_tab.set(BackTab::Word);
        let i = *idx.read();
        let head = entries
            .read()
            .get(i)
            .map(|e| preferred_headword(e).to_string());
        if let Some(head) = head {
            spawn(async move {
                kanji.set(kanji_for(&head).await.unwrap_or_default());
                examples.set(examples_for(&head, 5).await.unwrap_or_default());
            });
        }
    };

    let rate = move |r: u8| {
        // Each card is rated once, and only with its answer showing. Queued
        // shortcut presses run after the card has moved on, so they must not
        // land on the next card before it has been seen.
        if *rating_in_flight.peek() || !*show_back.peek() {
            return;
        }
        let cards = due_cards.read();
        let i = *idx.read();
        let Some(card) = cards.get(i).cloned() else {
            return;
        };
        let entry_for_label = entries.read().get(i).cloned();
        if let Some(entry) = entry_for_label.clone() {
            results.with_mut(|v| {
                v.push(SessionResult {
                    id: card.id.clone(),
                    sequence: card.sequence,
                    direction: card.direction,
                    entry,
                    rating: r,
                })
            });
        }
        if *drill.peek() {
            // A drill counts per card and a word takes its best card's count,
            // so walking away mid-drill still credits what was practised.
            drill_log::record_drill(&card, r);
            idx.with_mut(|i| *i += 1);
            show_back.set(false);
            return;
        }
        rating_in_flight.set(true);
        spawn(async move {
            let settings = load_settings();
            let outcome = apply_review(&card, rating_from_u8(r), now_ms(), &settings);
            let row = drill_log::answer(&card, r, LogKind::Review);
            let card_id = card.id.clone();
            match outcome {
                ReviewOutcome::Rescheduled(c) => {
                    if let Err(e) = put_card_logged(&c, &row).await {
                        warn!("put_card({card_id}) after review failed: {e}");
                    } else {
                        schedule_sync();
                    }
                }
                ReviewOutcome::Graduated(c) => {
                    if let Err(e) = put_card_logged(&c, &row).await {
                        warn!("put_card({card_id}) on graduation failed: {e}");
                    } else {
                        schedule_sync();
                    }
                    let label = entry_for_label
                        .as_ref()
                        .map(|e| preferred_headword(e).to_string())
                        .unwrap_or_else(|| format!("seq {}", card.sequence));
                    graduated_msg.set(Some(format!(
                        "「{}」 ({}) graduated — kept in Word List, hidden from Review.",
                        label,
                        card.direction.as_str(),
                    )));
                }
            }
            idx.with_mut(|i| *i += 1);
            show_back.set(false);
            rating_in_flight.set(false);
            // If session ended, compute next-due across all active cards. The
            // check is taken before the await: a read guard held across it
            // would panic on any `due_cards` write in the meantime.
            let session_over = *idx.peek() >= due_cards.peek().len();
            if session_over {
                match get_all_cards().await {
                    Ok(mut all) => {
                        all.retain(|c| !c.is_kanji());
                        next_due.set(next_due_message(&all, now_ms()));
                    }
                    Err(e) => warn!("get_all_cards for next-due summary failed: {e}"),
                }
            }
        });
    };

    use_review_shortcuts(
        move || *started.peek() && *idx.peek() < due_cards.peek().len(),
        show_back,
        reveal_answer,
        rate,
    );

    let cards = due_cards.read();
    let i = *idx.read();
    let current = cards.get(i);
    let entries_r = entries.read();
    let current_entry = entries_r.get(i).cloned();
    let due_count = cards.len();

    if !*cards_loaded.read() {
        return rsx! { div { class: "loading", "Loading…" } };
    }
    let ready = *entries_ready.read();

    let new_count = *staging_count.read();
    let due_ready_label = if due_count == 1 {
        format!("{due_count} card ready")
    } else {
        format!("{due_count} cards ready")
    };
    let new_label = if new_count == 1 {
        format!("{new_count} new word to learn")
    } else {
        format!("{new_count} new words to learn")
    };

    rsx! {
        div {
            if !*started.read() {
                div { class: "stat-grid",
                    div { class: "stat-card accent",
                        div { class: "stat-value", "{due_count}" }
                        div { class: "stat-label", "Due now" }
                    }
                    div { class: "stat-card warn",
                        div { class: "stat-value", "{new_count}" }
                        div { class: "stat-label", "Staged" }
                    }
                    div { class: "stat-card",
                        div { class: "stat-value", "{i.min(due_count)}" }
                        div { class: "stat-label", "Reviewed this session" }
                    }
                }
            }


            if let Some(msg) = graduated_msg.read().clone() {
                div { class: "card", style: "background: var(--green); color: var(--on-accent); border-color: var(--green);", "{msg}" }
            }
            if !skipped.read().is_empty() {
                div { class: "card", style: "background: var(--yellow); color: var(--on-accent); border-color: var(--yellow);",
                    "Skipped {skipped.read().len()} card(s) — no longer in the dictionary: "
                    strong { "{skipped.read().join(\"、\")}" }
                }
            }

            if !*started.read() {
                if due_count > 0 {
                    div { class: "empty-state",
                        div { class: "glyph", Icon { name: IconName::Play, size: 44 } }
                        div { class: "headline", "{due_ready_label}" }
                        div { class: "helper", "Start your review session." }
                        button { class: "primary", disabled: !ready, onclick: start_review,
                            if ready { "Start Review" } else { "Loading dictionary…" }
                        }
                    }
                } else if new_count > 0 {
                    div { class: "empty-state",
                        div { class: "glyph", Icon { name: IconName::Sparkles, size: 44 } }
                        div { class: "headline", "{new_label}" }
                        div { class: "helper", "Pick which ones to start learning." }
                        button { class: "primary", onclick: move |_| { navigator().push(Route::New {}); }, "Go to New Words" }
                    }
                } else {
                    div { class: "empty-state",
                        div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                        div { class: "headline", "All caught up" }
                        div { class: "helper",
                            if let Some(m) = next_due.read().clone() { "{m}" } else { "No cards due right now." }
                        }
                    }
                }

                if !trouble.read().is_empty() {
                    {
                        let total = trouble.read().len();
                        let drill_label = if total > DRILL_BATCH {
                            format!("Drill next {DRILL_BATCH}")
                        } else {
                            "Drill these".to_string()
                        };
                        let shown = if *show_all_trouble.read() {
                            total
                        } else {
                            total.min(DRILL_BATCH)
                        };
                        let rows: Vec<(TroubleWord, Option<WordEntry>, u32)> = trouble
                            .read()
                            .iter()
                            .take(shown)
                            .map(|(w, e)| {
                                let n = drilled.read().get(&w.sequence).copied().unwrap_or(0);
                                (w.clone(), e.clone(), n)
                            })
                            .collect();
                        rsx! {
                        div { class: "card", style: "margin-top: 16px;",
                            div { class: "row", style: "justify-content: space-between; margin-bottom: 8px;",
                                div { class: "section-title", style: "margin-bottom: 0;", "Trouble words · {total}" }
                                button { class: "outline", onclick: start_drill,
                                    Icon { name: IconName::Play, size: 14 }
                                    "{drill_label}"
                                }
                            }
                            details { class: "info",
                                summary { Icon { name: IconName::Info, size: 13 } "How this works" }
                                ul {
                                    li { "Words you keep failing." }
                                    li { "A word leaves once a real review schedules it a month or more out." }
                                    li { "Drills take {DRILL_BATCH} words at a time, least-drilled first." }
                                    li { "Drilling never changes a word's schedule." }
                                }
                            }
                            div { class: "word-rows",
                                for (w, e, n) in rows {
                                    WordRow {
                                        key: "{w.sequence}",
                                        sequence: w.sequence,
                                        entry: e,
                                        note: (n > 0).then(|| format!("drilled {n}×")),
                                        badge: format!("failed {}×", w.fails),
                                        badge_class: "badge fail",
                                    }
                                }
                            }
                            if total > DRILL_BATCH {
                                div { style: "text-align: center; margin-top: 8px;",
                                    button { class: "link",
                                        onclick: move |_| show_all_trouble.toggle(),
                                        if *show_all_trouble.read() { "Show less" } else { "Show all ({total})" }
                                    }
                                }
                            }
                        }
                        }
                    }
                }
            } else if let Some(c) = current {
                {
                    let entry = current_entry;
                    let direction_label = direction_label(c.direction);
                    let is_drill = *drill.read();
                    let reading = entry.as_ref().map(|e| primary_reading(e).to_string()).unwrap_or_default();
                    let front_word = entry
                        .as_ref()
                        .map(|e| preferred_headword(e).to_string())
                        .unwrap_or_else(|| format!("(seq {})", c.sequence));
                    // When the word is shown as its kana reading (kana-preferred),
                    // surface the kanji writing underneath in smaller text.
                    let sub_kanji = if front_word == reading {
                        entry
                            .as_ref()
                            .and_then(|e| e.kanji_forms.first())
                            .map(|k| k.text.clone())
                    } else {
                        None
                    };
                    let recall_glosses: Vec<String> = entry
                        .as_ref()
                        .map(|e| {
                            e.senses
                                .iter()
                                .take(3)
                                .map(|s| {
                                    s.glosses
                                        .iter()
                                        .map(|g| g.text.clone())
                                        .collect::<Vec<_>>()
                                        .join("; ")
                                })
                                .filter(|g| !g.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();
                    let show_back_v = *show_back.read();
                    let in_flight = *rating_in_flight.read();
                    let is_recall = matches!(c.direction, CardDirection::Recall);
                    // Recognition cards test going from the written word to its
                    // reading, so the reading (and, for kana-preferred headwords,
                    // the underlying kanji spelling) must stay hidden on the front
                    // face for every word, not just kanji/hiragana combos like
                    // 食べる, until the answer is revealed.
                    let hide_reading = !show_back_v;

                    rsx! {
                        div { class: "review-card",
                            div { class: "row", style: "justify-content: space-between;",
                                span { class: "muted", "Card {i + 1} of {due_count}" }
                                span { class: "row",
                                    if is_drill {
                                        span { class: "badge fail", "Drill" }
                                    }
                                    span { class: "badge", "{direction_label}" }
                                }
                            }

                            div { class: "face",
                                if !show_back_v && is_recall {
                                    div { class: "recall-glosses",
                                        if recall_glosses.is_empty() {
                                            div { class: "muted", "No definition available." }
                                        } else {
                                            for (gi, g) in recall_glosses.iter().enumerate() {
                                                div { style: "margin: 4px 0;",
                                                    span { class: "num", "{gi + 1}." }
                                                    "{g}"
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    div { class: "word", "{front_word}" }
                                    if !hide_reading {
                                        if let Some(k) = sub_kanji {
                                            div { class: "kanji-sub", "{k}" }
                                        } else if !reading.is_empty() && reading != front_word {
                                            div { class: "reading", "{reading}" }
                                        }
                                    }
                                }
                            }

                            if show_back_v {
                                BackContent {
                                    entry,
                                    kanji: kanji.read().clone(),
                                    examples: examples.read().clone(),
                                    word: front_word,
                                    tab: *back_tab.read(),
                                    on_tab: EventHandler::new(move |t| back_tab.set(t)),
                                }
                            }

                            div { style: "margin-top: 16px;",
                                if !show_back_v {
                                    ShowAnswerButton { on_reveal: move |_| reveal_answer() }
                                } else {
                                    RateButtons { disabled: in_flight, on_rate: rate }
                                }
                            }
                        }
                    }
                }

            } else {
                {
                    let res = results.read();
                    let (forgot, remembered): (Vec<SessionResult>, Vec<SessionResult>) =
                        res.iter().cloned().partition(|r| r.rating == 1);
                    let headline = if *drill.read() { "Drill complete!" } else { "Review complete!" };
                    rsx! {
                        div { class: "empty-state",
                            div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                            div { class: "headline", "{headline}" }
                            div { class: "helper",
                                "{remembered.len()} remembered · {forgot.len()} forgot"
                                if let Some(m) = next_due.read().clone() { " · {m}" }
                            }
                        }
                        if !res.is_empty() {
                            div { class: "recap-grid",
                                RecapList {
                                    title: format!("Forgot · {}", forgot.len()),
                                    title_class: "section-title error",
                                    empty: "Nothing forgotten — clean session.",
                                    results: forgot,
                                }
                                RecapList {
                                    title: format!("Remembered · {}", remembered.len()),
                                    title_class: "section-title ok",
                                    empty: "Nothing remembered this time.",
                                    results: remembered,
                                }
                            }
                        }
                        div { style: "text-align: center; margin-top: 16px;",
                            button { class: "primary", onclick: move |_| load_session(), "Done" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WordRow(
    sequence: u32,
    entry: Option<WordEntry>,
    /// Small muted text before the badge, e.g. "drilled 2×".
    note: Option<String>,
    badge: String,
    badge_class: &'static str,
) -> Element {
    let (head, reading, gloss) = match &entry {
        Some(e) => {
            let head = preferred_headword(e).to_string();
            let reading = primary_reading(e).to_string();
            let reading = if reading == head {
                String::new()
            } else {
                reading
            };
            (head, reading, short_gloss(e))
        }
        None => (format!("(seq {sequence})"), String::new(), String::new()),
    };
    rsx! {
        div { class: "word-row",
            span { class: "w", "{head}" }
            if !reading.is_empty() {
                span { class: "r", "{reading}" }
            }
            span { class: "g", title: "{gloss}", "{gloss}" }
            if let Some(n) = note {
                span { class: "n", "{n}" }
            }
            span { class: "{badge_class}", "{badge}" }
        }
    }
}

#[component]
fn RecapList(
    title: String,
    title_class: &'static str,
    empty: &'static str,
    results: Vec<SessionResult>,
) -> Element {
    rsx! {
        div { class: "card",
            div { class: "{title_class}", "{title}" }
            if results.is_empty() {
                div { class: "muted", style: "font-size: 13px;", "{empty}" }
            } else {
                div { class: "word-rows",
                    for r in results {
                        {
                            let failed = r.rating == 1;
                            let badge = if failed {
                                direction_label(r.direction).to_string()
                            } else {
                                format!("{} · {}", direction_label(r.direction), rating_label(r.rating))
                            };
                            let badge_class = if failed { "badge fail" } else { "badge" };
                            rsx! {
                                WordRow {
                                    key: "{r.id}",
                                    sequence: r.sequence,
                                    entry: Some(r.entry),
                                    badge,
                                    badge_class,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn BackContent(
    entry: Option<WordEntry>,
    kanji: Vec<kanjidic_types::KanjiEntry>,
    examples: Vec<examples_types::ExampleEntry>,
    word: String,
    tab: BackTab,
    on_tab: EventHandler<BackTab>,
) -> Element {
    let kanji_visible = !kanji.is_empty();
    rsx! {
        div { class: "subtabs",
            TabButton { active: tab == BackTab::Word,     onclick: move |_| on_tab.call(BackTab::Word),     label: "Word" }
            if kanji_visible {
                TabButton { active: tab == BackTab::Kanji,    onclick: move |_| on_tab.call(BackTab::Kanji),    label: "Kanji" }
            }
            TabButton { active: tab == BackTab::Examples, onclick: move |_| on_tab.call(BackTab::Examples), label: "Examples" }
        }
        match tab {
            BackTab::Word => rsx! {
                div {
                    if let Some(e) = entry {
                        for (si, sense) in e.senses.iter().take(3).enumerate() {
                            {
                                let g = sense.glosses.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("; ");
                                if g.is_empty() {
                                    rsx! { div {} }
                                } else {
                                    rsx! {
                                        div { style: "margin: 8px 0;",
                                            if !sense.pos.is_empty() {
                                                div { class: "pos", "{pos_list(&sense.pos)}" }
                                            }
                                            div { class: "gloss",
                                                span { class: "muted", "{si + 1}. " }
                                                "{g}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            BackTab::Kanji => rsx! {
                div {
                    for k in kanji {
                        div { class: "kanji-row",
                            span { class: "literal", "{k.literal}" }
                            div { class: "meta",
                                if !k.on_readings.is_empty() {
                                    span { class: "muted", "On: {k.on_readings.join(\"、\")}" }
                                }
                                if !k.kun_readings.is_empty() {
                                    span { class: "muted", "Kun: {k.kun_readings.join(\"、\")}" }
                                }
                                span { "{k.meanings.iter().take(3).cloned().collect::<Vec<_>>().join(\", \")}" }
                            }
                        }
                    }
                }
            },
            BackTab::Examples => rsx! {
                div {
                    if examples.is_empty() {
                        div { class: "empty", "No examples found." }
                    } else {
                        for ex in examples {
                            div { class: "example-row",
                                div { class: "jp", ExampleJp { sentence: ex.japanese.clone(), word: word.clone() } }
                                div { class: "en", "{ex.english}" }
                            }
                        }
                    }
                }
            },
        }
    }
}

#[component]
fn ExampleJp(sentence: String, word: String) -> Element {
    if let Some(idx) = sentence.find(&word) {
        let before = &sentence[..idx];
        let after = &sentence[idx + word.len()..];
        rsx! {
            "{before}"
            mark { "{word}" }
            "{after}"
        }
    } else {
        rsx! { "{sentence}" }
    }
}
