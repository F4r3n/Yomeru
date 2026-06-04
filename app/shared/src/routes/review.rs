use dioxus::prelude::*;
use jmdict_types::WordEntry;

use crate::components::pos_list;
use crate::dict::{
    examples_for, kanji_for, lookup_by_sequence, preferred_headword, primary_reading,
};
use crate::idb::{
    delete_card_by_id, get_all_cards, get_due_cards, get_staging_cards, promote_card, put_card,
};
use crate::settings::load as load_settings;
use crate::srs::{ReviewOutcome, apply_review, now_ms, rating_from_u8};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{CardDirection, CardStatus, SrsCard};

#[derive(Clone, Copy, PartialEq)]
enum BackTab {
    Word,
    Kanji,
    Examples,
}

fn shuffle<T>(mut v: Vec<T>) -> Vec<T> {
    // Fisher-Yates with Math.random() — good enough for review-order randomness.
    for i in (1..v.len()).rev() {
        let r = (js_sys::Math::random() * (i as f64 + 1.0)) as usize;
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

fn next_due_message(cards: &[SrsCard], now: f64) -> Option<String> {
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

#[component]
pub fn ReviewTab() -> Element {
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
    let mut dict_ready = use_signal(|| false);

    let load_session = move || {
        spawn(async move {
            dict_ready.set(true);
            let settings = load_settings();
            let now = now_ms();
            let due = get_due_cards(now).await.unwrap_or_default();
            let limited: Vec<_> = due
                .into_iter()
                .take(settings.max_session_cards as usize)
                .collect();
            let (kept, ents, skips) = attach_entries(limited).await;
            // Pair, shuffle as a single permutation, then split back so cards
            // and entries stay aligned.
            let mut paired: Vec<_> = kept.into_iter().zip(ents.into_iter()).collect();
            paired = shuffle(paired);
            let (kept_final, ents_final) = paired.into_iter().unzip();
            due_cards.set(kept_final);
            entries.set(ents_final);
            skipped.set(skips);
            idx.set(0);
            show_back.set(false);
            started.set(false);
            graduated_msg.set(None);
            next_due.set(None);
            let staging = get_staging_cards().await.unwrap_or_default();
            // Unique by sequence — recognition and recall siblings count once.
            let mut seen = std::collections::HashSet::new();
            let n = staging
                .into_iter()
                .filter(|c| seen.insert(c.sequence))
                .count();
            staging_count.set(n);
        });
    };

    // Reload on mount and whenever a sync lands, but don't yank a session the
    // user has already started.
    use_reload_on_sync(move || {
        if !*started.peek() {
            load_session();
        }
    });

    let start_review = move |_| started.set(true);

    let promote_and_review = move |_| {
        spawn(async move {
            let settings = load_settings();
            let staging = match get_staging_cards().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("get_staging_cards in promote_and_review failed: {e}");
                    return;
                }
            };
            let mut seen = std::collections::HashSet::new();
            let seqs: Vec<u32> = staging
                .into_iter()
                .filter(|c| seen.insert(c.sequence))
                .map(|c| c.sequence)
                .collect();
            let n = (seqs.len()).min(settings.max_session_cards as usize);
            let mut promoted = 0usize;
            for seq in seqs.iter().take(n).copied() {
                if let Err(e) = promote_card(seq).await {
                    warn!("promote_card(seq={seq}) in promote_and_review failed: {e}");
                    continue;
                }
                promoted += 1;
            }
            if promoted > 0 {
                schedule_sync();
            }
            started.set(true);
            load_session();
        });
    };

    let reveal_answer = move |_| {
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
        let cards = due_cards.read();
        let i = *idx.read();
        let Some(card) = cards.get(i).cloned() else {
            return;
        };
        let entry_for_label = entries.read().get(i).cloned();
        spawn(async move {
            let settings = load_settings();
            let outcome = apply_review(&card, rating_from_u8(r), now_ms(), &settings);
            let card_id = card.id.clone();
            match outcome {
                ReviewOutcome::Rescheduled(c) => {
                    if let Err(e) = put_card(&c).await {
                        warn!("put_card({card_id}) after review failed: {e}");
                    } else {
                        schedule_sync();
                    }
                }
                ReviewOutcome::Graduated => {
                    if let Err(e) = delete_card_by_id(&card_id).await {
                        warn!("delete_card_by_id({card_id}) on graduation failed: {e}");
                    } else {
                        schedule_sync();
                    }
                    let label = entry_for_label
                        .as_ref()
                        .map(|e| preferred_headword(e).to_string())
                        .unwrap_or_else(|| format!("seq {}", card.sequence));
                    graduated_msg.set(Some(format!(
                        "「{}」 ({}) graduated — removed from review queue.",
                        label,
                        match card.direction {
                            CardDirection::Recognition => "recognition",
                            CardDirection::Recall => "recall",
                        }
                    )));
                }
            }
            idx.with_mut(|i| *i += 1);
            show_back.set(false);
            // If session ended, compute next-due across all active cards.
            let cards = due_cards.read();
            if *idx.read() >= cards.len() {
                match get_all_cards().await {
                    Ok(all) => next_due.set(next_due_message(&all, now_ms())),
                    Err(e) => warn!("get_all_cards for next-due summary failed: {e}"),
                }
            }
        });
    };

    let cards = due_cards.read();
    let i = *idx.read();
    let current = cards.get(i);
    let entries_r = entries.read();
    let current_entry = entries_r.get(i).cloned();
    let due_count = cards.len();

    if !*dict_ready.read() {
        return rsx! { div { class: "loading", "Loading dictionary…" } };
    }

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
            div { class: "page-header",
                div {
                    h2 { "Review" }
                    div { class: "subtitle", "Spaced-repetition review session." }
                }
            }

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
                        div { class: "glyph", "▶" }
                        div { class: "headline", "{due_ready_label}" }
                        div { class: "helper", "Start your review session." }
                        button { class: "primary", onclick: start_review, "Start Review" }
                    }
                } else if new_count > 0 {
                    div { class: "empty-state",
                        div { class: "glyph", "✦" }
                        div { class: "headline", "{new_label}" }
                        div { class: "helper", "Promote them into the SRS queue." }
                        button { class: "primary", onclick: promote_and_review, "Add new words" }
                    }
                } else {
                    div { class: "empty-state",
                        div { class: "glyph", "✓" }
                        div { class: "headline", "All caught up" }
                        div { class: "helper",
                            if let Some(m) = next_due.read().clone() { "{m}" } else { "No cards due right now." }
                        }
                    }
                }
            } else if let Some(c) = current {
                {
                    let entry = current_entry;
                    let direction_label = match c.direction {
                        CardDirection::Recognition => "Recognition",
                        CardDirection::Recall => "Recall",
                    };
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
                    let is_recall = matches!(c.direction, CardDirection::Recall);

                    rsx! {
                        div { class: "review-card",
                            div { class: "row", style: "justify-content: space-between;",
                                span { class: "muted", "Card {i + 1} of {due_count}" }
                                span { class: "badge", "{direction_label}" }
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
                                    if let Some(k) = sub_kanji.clone() {
                                        div { class: "kanji-sub", "{k}" }
                                    } else if !reading.is_empty() && reading != front_word {
                                        div { class: "reading", "{reading}" }
                                    }
                                }
                            }

                            if show_back_v {
                                BackContent {
                                    entry: entry.clone(),
                                    kanji: kanji.read().clone(),
                                    examples: examples.read().clone(),
                                    word: front_word.clone(),
                                    tab: *back_tab.read(),
                                    on_tab: EventHandler::new(move |t| back_tab.set(t)),
                                }
                            }

                            div { style: "margin-top: 16px;",
                                if !show_back_v {
                                    button { class: "primary", style: "width: 100%;",
                                        onclick: reveal_answer, "Show answer"
                                    }
                                } else {
                                    div { class: "rate-grid",
                                        button { class: "danger",  onclick: move |_| (rate)(1), "Again" }
                                        button { class: "warning", onclick: move |_| (rate)(2), "Hard" }
                                        button { class: "success", onclick: move |_| (rate)(3), "Good" }
                                        button { class: "primary", onclick: move |_| (rate)(4), "Easy" }
                                    }
                                }
                            }
                        }
                    }
                }

            } else {
                div { class: "empty-state",
                    div { class: "glyph", "✓" }
                    div { class: "headline", "Review complete!" }
                    div { class: "helper",
                        if let Some(m) = next_due.read().clone() { "{m}" } else { "Great work." }
                    }
                    button { class: "primary", onclick: move |_| load_session(), "Done" }
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

#[component]
fn TabButton(active: bool, onclick: EventHandler<MouseEvent>, label: &'static str) -> Element {
    let class = if active { "active" } else { "" };
    rsx! {
        button { class: "{class}", onclick: move |e| onclick.call(e), "{label}" }
    }
}
