use dioxus::prelude::*;
use jmdict_types::WordEntry;

use crate::components::pos_list;
use crate::dict::{self, examples_for, kanji_for, primary_reading};
use crate::idb::{
    delete_card_by_id, get_all_cards, get_due_cards, get_staging_cards, promote_card, put_card,
};
use crate::settings::load as load_settings;
use crate::srs::{apply_review, now_ms, rating_from_u8, ReviewOutcome};
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

async fn attach_entries(
    cards: Vec<SrsCard>,
) -> (Vec<SrsCard>, Vec<WordEntry>, Vec<String>) {
    if cards.is_empty() {
        return (vec![], vec![], vec![]);
    }
    let words: Vec<String> = cards.iter().map(|c| c.word.clone()).collect();
    let all_hits = dict::lookup_many(&words).await.unwrap_or_default();
    let mut kept = Vec::new();
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    for (c, hits) in cards.into_iter().zip(all_hits.into_iter()) {
        let entry = hits.into_iter().next();
        if entry.is_none() && matches!(c.direction, CardDirection::Recall) {
            // Recall front needs glosses; without them the card is unreviewable.
            skipped.push(format!("{} (recall)", c.word));
            continue;
        }
        entries.push(entry.unwrap_or_else(|| WordEntry {
            sequence: 0,
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
            let limited: Vec<_> =
                due.into_iter().take(settings.max_session_cards as usize).collect();
            let (kept, ents, skips) = attach_entries(limited).await;
            // Pair, shuffle as a single permutation, then split back so cards
            // and entries stay aligned.
            let mut paired: Vec<_> = kept.into_iter().zip(ents.into_iter()).collect();
            paired = shuffle(paired);
            let (kept_final, ents_final): (Vec<_>, Vec<_>) = paired.into_iter().unzip();
            due_cards.set(kept_final);
            entries.set(ents_final);
            skipped.set(skips);
            idx.set(0);
            show_back.set(false);
            started.set(false);
            graduated_msg.set(None);
            next_due.set(None);
            let staging = get_staging_cards().await.unwrap_or_default();
            // Unique by word.
            let mut seen = std::collections::HashSet::new();
            let n = staging
                .into_iter()
                .filter(|c| seen.insert(c.word.clone()))
                .count();
            staging_count.set(n);
        });
    };

    use_effect(move || load_session());

    let start_review = move |_| started.set(true);

    let promote_and_review = move |_| {
        spawn(async move {
            let settings = load_settings();
            let staging = get_staging_cards().await.unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            let words: Vec<_> = staging
                .into_iter()
                .filter(|c| seen.insert(c.word.clone()))
                .map(|c| c.word)
                .collect();
            let n = (words.len()).min(settings.max_session_cards as usize);
            for w in &words[..n] {
                let _ = promote_card(w).await;
            }
            started.set(true);
            load_session();
        });
    };

    let reveal_answer = move |_| {
        show_back.set(true);
        back_tab.set(BackTab::Word);
        let cards = due_cards.read();
        let i = *idx.read();
        if let Some(c) = cards.get(i).cloned() {
            spawn(async move {
                kanji.set(kanji_for(&c.word).await.unwrap_or_default());
                examples.set(examples_for(&c.word, 5).await.unwrap_or_default());
            });
        }
    };

    let rate = move |r: u8| {
        let cards = due_cards.read().clone();
        let i = *idx.read();
        let Some(card) = cards.get(i).cloned() else { return };
        spawn(async move {
            let settings = load_settings();
            let outcome = apply_review(&card, rating_from_u8(r), now_ms(), &settings);
            match outcome {
                ReviewOutcome::Rescheduled(c) => {
                    let _ = put_card(&c).await;
                }
                ReviewOutcome::Graduated => {
                    let _ = delete_card_by_id(&card.id).await;
                    graduated_msg.set(Some(format!(
                        "「{}」 ({}) graduated — removed from review queue.",
                        card.word,
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
                let all = get_all_cards().await.unwrap_or_default();
                next_due.set(next_due_message(&all, now_ms()));
            }
        });
    };

    let cards = due_cards.read();
    let i = *idx.read();
    let current = cards.get(i).cloned();
    let entries_r = entries.read();
    let current_entry = entries_r.get(i).cloned();
    let due_count = cards.len();

    if !*dict_ready.read() {
        return rsx! { div { class: "loading", "Loading dictionary…" } };
    }

    let stats = if *staging_count.read() > 0 {
        format!(
            "{due_count} card{} due · {} new",
            if due_count == 1 { "" } else { "s" },
            *staging_count.read()
        )
    } else {
        format!(
            "{due_count} card{} due",
            if due_count == 1 { "" } else { "s" }
        )
    };

    rsx! {
        div { class: "muted", style: "font-size: 13px; margin-bottom: 12px;", "{stats}" }

        if let Some(msg) = graduated_msg.read().clone() {
            div { class: "card", style: "background: var(--green); color: var(--bg);", "{msg}" }
        }
        if !skipped.read().is_empty() {
            div { class: "card", style: "background: var(--yellow); color: var(--bg);",
                "Skipped {skipped.read().len()} card(s) — no longer in the dictionary: "
                strong { "{skipped.read().join(\"、\")}" }
            }
        }

        if !*started.read() {
            div { class: "empty",
                if due_count > 0 {
                    p { "{due_count} card(s) ready for review." }
                    button { class: "primary", onclick: start_review, "Start Review" }
                } else if *staging_count.read() > 0 {
                    p { "{*staging_count.read()} new word(s) ready to learn." }
                    button { class: "primary", onclick: promote_and_review, "Add new words" }
                } else {
                    p { "No cards due right now." }
                    if let Some(m) = next_due.read().clone() {
                        p { style: "color: var(--accent); margin-top: 8px;", "{m}" }
                    }
                }
            }
        } else if current.is_none() {
            div { class: "empty",
                p { "Review complete!" }
                if let Some(m) = next_due.read().clone() {
                    p { style: "color: var(--accent); margin-top: 8px;", "{m}" }
                }
                button { class: "primary", onclick: move |_| load_session(), "Done" }
            }
        } else {
            {
                let c = current.unwrap();
                let entry = current_entry;
                let direction_label = match c.direction {
                    CardDirection::Recognition => "Recognition",
                    CardDirection::Recall => "Recall",
                };
                let reading = entry.as_ref().map(|e| primary_reading(e).to_string()).unwrap_or_default();
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
                    div { class: "card",
                        div { class: "row", style: "justify-content: space-between;",
                            span { class: "muted", "{i + 1} / {due_count}" }
                            span { class: "badge", "{direction_label}" }
                        }

                        div { style: "padding: 24px 8px; text-align: center;",
                            if !show_back_v && is_recall {
                                if recall_glosses.is_empty() {
                                    div { class: "muted", "No definition available." }
                                } else {
                                    for (gi, g) in recall_glosses.iter().enumerate() {
                                        div { style: "margin: 4px 0;",
                                            span { class: "muted", "{gi + 1}. " }
                                            "{g}"
                                        }
                                    }
                                }
                            } else {
                                div { style: "font-size: 28px;",
                                    "{c.word}"
                                    if !reading.is_empty() && reading != c.word {
                                        span { class: "muted", style: "font-size: 14px; margin-left: 12px;", "{reading}" }
                                    }
                                }
                            }
                        }

                        if show_back_v {
                            BackContent {
                                entry: entry.clone(),
                                kanji: kanji.read().clone(),
                                examples: examples.read().clone(),
                                word: c.word.clone(),
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
                                div { class: "row", style: "gap: 8px;",
                                    button { class: "danger",  style: "flex: 1;", onclick: move |_| (rate.clone())(1), "Again" }
                                    button {                  style: "flex: 1;", onclick: move |_| (rate.clone())(2), "Hard" }
                                    button { class: "success", style: "flex: 1;", onclick: move |_| (rate.clone())(3), "Good" }
                                    button { class: "primary", style: "flex: 1;", onclick: move |_| (rate.clone())(4), "Easy" }
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
        div { class: "row", style: "border-bottom: 1px solid var(--border); margin: 8px 0;",
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
                                        div { style: "margin: 6px 0;",
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
                        div { class: "row", style: "padding: 6px 0; border-bottom: 1px solid var(--border);",
                            span { style: "font-size: 28px; min-width: 48px;", "{k.literal}" }
                            div { class: "col", style: "gap: 2px;",
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
                            div { style: "padding: 6px 0; border-bottom: 1px solid var(--border);",
                                div { ExampleJp { sentence: ex.japanese.clone(), word: word.clone() } }
                                div { class: "muted", "{ex.english}" }
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
            mark { style: "background: var(--yellow); color: var(--bg); padding: 0 2px;", "{word}" }
            "{after}"
        }
    } else {
        rsx! { "{sentence}" }
    }
}

#[component]
fn TabButton(active: bool, onclick: EventHandler<MouseEvent>, label: &'static str) -> Element {
    let class = if active { "tab active" } else { "tab" };
    rsx! {
        button { class: "{class}", onclick: move |e| onclick.call(e), "{label}" }
    }
}
