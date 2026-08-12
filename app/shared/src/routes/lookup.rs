use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use gloo_storage::{LocalStorage, Storage};
use gloo_timers::future::TimeoutFuture;
use jmdict_types::WordEntry;

use crate::app::Route;
use crate::components::EntryCard;
use crate::dict::{self, examples_for, kanji_for, primary_headword};
use crate::idb::{bump_priority, get_cards_by_sequence, put_cards, reset_card};
use crate::romaji::{is_romaji, romaji_to_hiragana};
use crate::srs::now_ms;
use crate::sync::schedule_sync;
use crate::types::{CardDirection, CardStatus, SrsCard};

const HISTORY_KEY: &str = "lookup_history";
const HISTORY_MAX: usize = 10;

#[derive(Clone, Copy, PartialEq)]
enum ExtraTab {
    Kanji,
    Examples,
}

/// Shared lookup state across the list and the (currently empty) child
/// route components. Currently just `card_status` so `+ Add` from the
/// expansion panel can flip the badge on the result card too. Keyed by
/// JMdict ent_seq so kanji-vs-kana display swaps don't desync the badge.
#[derive(Clone, Copy)]
struct LookupShared {
    card_status: Signal<HashMap<u32, CardStatus>>,
}

// ── helpers ──────────────────────────────────────────────────────────

fn load_history() -> Vec<String> {
    LocalStorage::get::<Vec<String>>(HISTORY_KEY).unwrap_or_default()
}

fn push_history(mut h: Vec<String>, term: &str) -> Vec<String> {
    h.retain(|x| x != term);
    h.insert(0, term.to_owned());
    h.truncate(HISTORY_MAX);
    let _ = LocalStorage::set(HISTORY_KEY, &h);
    h
}

/// Handles the two non-destructive "Add" outcomes: create fresh staging
/// cards for a word not yet in the list, or bump the priority of one
/// already staged. Never called when the word is `Active` — that's
/// `on_reset`'s job instead.
async fn add_or_bump(
    sequence: u32,
    mut card_status: Signal<HashMap<u32, CardStatus>>,
    mut card_priority: Signal<HashMap<u32, u32>>,
) {
    let existing = match get_cards_by_sequence(sequence).await {
        Ok(e) => e,
        Err(e) => {
            warn!("get_cards_by_sequence({sequence}) failed: {e}");
            return;
        }
    };
    if existing.is_empty() {
        let now = now_ms();
        let cards = vec![
            SrsCard::new(sequence, CardDirection::Recognition, now),
            SrsCard::new(sequence, CardDirection::Recall, now),
        ];
        if let Err(e) = put_cards(&cards).await {
            warn!("put_cards(seq={sequence}) on add failed: {e}");
            return;
        }
        schedule_sync();
        card_priority.with_mut(|m| {
            m.insert(sequence, 0);
        });
    } else if !existing.iter().any(|c| matches!(c.status, CardStatus::Active)) {
        if let Err(e) = bump_priority(sequence).await {
            warn!("bump_priority(seq={sequence}) failed: {e}");
            return;
        }
        schedule_sync();
        // Re-fetch rather than compute locally so the displayed value always
        // matches what `bump_priority`'s cap actually wrote.
        let refreshed = get_cards_by_sequence(sequence).await.unwrap_or_default();
        if let Some(p) = refreshed.iter().map(|c| c.priority).max() {
            card_priority.with_mut(|m| {
                m.insert(sequence, p);
            });
        }
    }
    card_status.with_mut(|m| {
        m.insert(sequence, CardStatus::Staging);
    });
}

fn status_of(cards: &[SrsCard]) -> Option<CardStatus> {
    if cards.iter().any(|c| matches!(c.status, CardStatus::Active)) {
        Some(CardStatus::Active)
    } else if !cards.is_empty() {
        Some(CardStatus::Staging)
    } else {
        None
    }
}

// ── Layout (shared by /lookup and /lookup/:word) ─────────────────────
//
// Both routes render the same single-column list. The URL just tells the
// list which card to expand inline. Outlet renders nothing (child routes
// are no-ops) but it's wired up so the routes resolve correctly.

#[component]
pub fn LookupLayout() -> Element {
    let card_status = use_signal(HashMap::<u32, CardStatus>::new);
    use_context_provider(|| LookupShared { card_status });
    rsx! {
        LookupListPane {}
        Outlet::<Route> {}
    }
}

#[component]
pub fn LookupEmpty() -> Element {
    rsx! { Fragment {} }
}

#[component]
pub fn LookupDetailPane(word: String) -> Element {
    // The list pane reads the URL directly via use_route() to know which
    // card to expand — this component just owns the route.
    let _ = word;
    rsx! { Fragment {} }
}

// ── List + inline expansion ──────────────────────────────────────────

#[component]
fn LookupListPane() -> Element {
    let LookupShared { mut card_status } = use_context::<LookupShared>();
    let nav = use_navigator();
    let current = use_route::<Route>();
    let selected_word: Option<String> = match current {
        Route::LookupDetail { word } => Some(word),
        _ => None,
    };
    let on_detail_at_render = selected_word.is_some();

    let query = use_signal(String::new);
    let mut last_target = use_signal(String::new);
    let mut entries = use_signal(Vec::<WordEntry>::new);
    let mut searching = use_signal(|| false);
    let mut searched = use_signal(|| false);
    let mut load_err = use_signal(|| Option::<String>::None);
    let mut history = use_signal(load_history);
    let mut card_priority = use_signal(HashMap::<u32, u32>::new);
    // Sequences whose Add/+Priority button was already clicked during the
    // current search — cleared on the next search so the button can't be
    // spammed while the async bump/create is in flight.
    let mut locked = use_signal(HashSet::<u32>::new);

    // Inline-expansion state — kanji + examples for the URL's selected word.
    let mut kanji_data = use_signal(Vec::<kanjidic_types::KanjiEntry>::new);
    let mut examples_data = use_signal(Vec::<examples_types::ExampleEntry>::new);
    let mut extra_tab = use_signal(|| ExtraTab::Kanji);
    let mut last_fetched = use_signal(String::new);

    // Re-fetch kanji/examples when the URL's word changes. Also lazy-load the
    // dictionary entry itself if no row in the list matches it (e.g. user
    // landed on /lookup/<word> via direct URL), so the expanded card has a
    // row to render under.
    if let Some(w) = selected_word.clone() {
        if *last_fetched.read() != w {
            last_fetched.set(w.clone());
            extra_tab.set(ExtraTab::Kanji);
            kanji_data.set(Vec::new());
            examples_data.set(Vec::new());
            let need_entry = !entries.read().iter().any(|e| primary_headword(e) == w);
            let w_for_aux = w.clone();
            spawn(async move {
                kanji_data.set(kanji_for(&w_for_aux).await.unwrap_or_default());
                examples_data.set(examples_for(&w_for_aux, 5).await.unwrap_or_default());
            });
            if need_entry {
                spawn(async move {
                    let results = dict::lookup(&w).await.unwrap_or_default();
                    if !results.is_empty() {
                        last_target.set(w);
                        entries.set(results);
                        searched.set(true);
                    }
                });
            }
        }
    } else if !last_fetched.read().is_empty() {
        last_fetched.set(String::new());
    }

    let run_lookup = move |q: String| {
        let q = q.trim();
        // A new search → collapse whatever card is currently open.
        if on_detail_at_render {
            nav.replace(Route::Lookup {});
        }
        if q.is_empty() {
            entries.set(Vec::new());
            searched.set(false);
            return;
        }
        let target = if is_romaji(q) {
            romaji_to_hiragana(q)
        } else {
            q.to_string()
        };
        last_target.set(target.clone());
        searching.set(true);
        let nav = nav;
        spawn(async move {
            let mut results = dict::lookup(&target).await.unwrap_or_else(|e| {
                load_err.set(Some(e));
                Vec::new()
            });
            if results.is_empty() {
                results = dict::lookup_prefix(&target, 30).await.unwrap_or_default();
            }
            if !results.is_empty() {
                let next = push_history(history.read().clone(), &target);
                history.set(next);
            }
            let mut statuses: HashMap<u32, CardStatus> = HashMap::new();
            let mut priorities: HashMap<u32, u32> = HashMap::new();

            //TODO: for each result we open IDB to see if the card exists.
            // It's slow, need to do it on batch
            for e in &results {
                let siblings = get_cards_by_sequence(e.sequence).await.unwrap_or_default();
                if let Some(s) = status_of(&siblings) {
                    statuses.insert(e.sequence, s);
                }
                if let Some(p) = siblings.iter().map(|c| c.priority).max() {
                    priorities.insert(e.sequence, p);
                }
            }
            card_status.set(statuses);
            card_priority.set(priorities);
            // A new set of results invalidates any button lock from the
            // previous search.
            locked.set(HashSet::new());
            let single_word = if results.len() == 1 {
                results.first().map(|v| primary_headword(v).to_string())
            } else {
                None
            };
            entries.set(results);
            searching.set(false);
            searched.set(true);
            if let Some(w) = single_word {
                nav.replace(Route::LookupDetail { word: w });
            }
        });
    };

    // 100ms debounce on the search input. `use_resource` cancels the prior
    // future (drops it at the sleep await) whenever `query` changes, so only
    // the last keystroke's lookup actually fires.
    let _debounced = use_resource(move || {
        let q = query.read().clone();
        let mut run_lookup = run_lookup;
        async move {
            TimeoutFuture::new(300).await;
            run_lookup(q);
        }
    });

    let on_input = {
        let mut query = query;
        move |evt: Event<FormData>| {
            query.set(evt.value());
        }
    };

    let on_history = {
        let mut query = query;
        let mut run_lookup = run_lookup;
        move |term: String| {
            query.set(term.clone());
            run_lookup(term);
        }
    };

    let on_clear_history = move |_| {
        LocalStorage::delete(HISTORY_KEY);
        history.set(Vec::new());
    };

    let on_add = move |sequence: u32| {
        locked.with_mut(|s| {
            s.insert(sequence);
        });
        spawn(async move { add_or_bump(sequence, card_status, card_priority).await });
    };

    let on_reset = move |sequence: u32| {
        spawn(async move {
            let confirmed = web_sys::window()
                .and_then(|w| {
                    w.confirm_with_message(
                        "Reset this word's SRS progress? This can't be undone.",
                    )
                    .ok()
                })
                .unwrap_or(false);
            if !confirmed {
                return;
            }
            if let Err(e) = reset_card(sequence, now_ms()).await {
                warn!("reset_card(seq={sequence}) failed: {e}");
                return;
            }
            schedule_sync();
            card_status.with_mut(|m| {
                m.insert(sequence, CardStatus::Active);
            });
        });
    };

    let q_trim = query.read().trim().to_string();
    let target = last_target.read().clone();
    let show_converted = is_romaji(&q_trim) && !target.is_empty() && target != q_trim;
    let result_count = entries.read().len();
    let result_label = if result_count == 1 {
        format!("{result_count} result")
    } else {
        format!("{result_count} results")
    };

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "Lookup" }
                    div { class: "subtitle", "Search JMdict by kanji, kana, or romaji." }
                }
                if result_count > 0 {
                    span { class: "pill", "{result_label}" }
                }
            }

            if let Some(err) = load_err.read().clone() {
                div { class: "card error", "Lookup error: {err}" }
            }

            div { class: "hero-search", style: "margin-bottom: 10px;",
                input {
                    r#type: "search",
                    placeholder: "Type a Japanese word… (kanji, kana, or romaji)",
                    value: "{query}",
                    autofocus: true,
                    oninput: on_input,
                }
            }
            if show_converted {
                div { class: "row", style: "gap: 6px; margin-bottom: 14px;",
                    span { class: "muted", style: "font-size: 12px;", "Reading as" }
                    span { class: "pill", "{target}" }
                }
            }

            if *searching.read() {
                div { class: "loading", "Searching…" }
            } else if !entries.read().is_empty() {
                for entry in entries.read().iter() {
                    {
                        let head = primary_headword(entry).to_string();
                        let status = card_status.read().get(&entry.sequence).copied();
                        let priority = card_priority.read().get(&entry.sequence).copied().unwrap_or(0);
                        let is_locked = locked.read().contains(&entry.sequence);
                        let expanded = selected_word.as_deref() == Some(&head);
                        let head_for_select = head.clone();
                        let head_for_close = head.clone();
                        let head_for_expansion = head.clone();
                        let on_select = if expanded {
                            // Already expanded — clicking collapses.
                            Some(EventHandler::new(move |_| {
                                nav.replace(Route::Lookup {});
                                let _ = &head_for_close;
                            }))
                        } else {
                            Some(EventHandler::new(move |_| {
                                nav.push(Route::LookupDetail { word: head_for_select.clone() });
                            }))
                        };
                        rsx! {
                            EntryCard {
                                entry: entry.clone(),
                                on_add: Some(EventHandler::new(on_add)),
                                on_reset: Some(EventHandler::new(on_reset)),
                                on_select,
                                status,
                                priority,
                                locked: is_locked,
                            }
                            if expanded {
                                ExpansionPanel {
                                    word: head_for_expansion,
                                    kanji: kanji_data.read().clone(),
                                    examples: examples_data.read().clone(),
                                    tab: *extra_tab.read(),
                                    on_tab: EventHandler::new(move |t| extra_tab.set(t)),
                                }
                            }
                        }
                    }
                }
            } else if *searched.read() {
                {
                    let label = if target.is_empty() { q_trim.clone() } else { target.clone() };
                    rsx! {
                        div { class: "empty-state",
                            div { class: "glyph", "⌕" }
                            div { class: "headline", "No entry found for 「{label}」" }
                            div { class: "helper", "Try a different spelling or check the romaji conversion." }
                        }
                    }
                }
            } else if !history.read().is_empty() {
                div { class: "card",
                    div { class: "row", style: "justify-content: space-between; margin-bottom: 10px;",
                        span { class: "section-title", style: "margin: 0;", "Recent searches" }
                        button { onclick: on_clear_history, "Clear" }
                    }
                    div { class: "chip-list",
                        for term in history.read().iter().cloned() {
                            {
                                let t = term.clone();
                                rsx! {
                                    button {
                                        class: "chip",
                                        onclick: move |_| {
                                            let mut h = on_history;
                                            h(t.clone());
                                        },
                                        "{term}"
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "empty-state",
                    div { class: "glyph", "あ" }
                    div { class: "headline", "Start typing to search" }
                    div { class: "helper", "e.g. 食べる, たべる, or taberu" }
                }
            }
        }
    }
}

// ── Inline expansion panel (sits directly under an expanded EntryCard) ───

#[component]
fn ExpansionPanel(
    word: String,
    kanji: Vec<kanjidic_types::KanjiEntry>,
    examples: Vec<examples_types::ExampleEntry>,
    tab: ExtraTab,
    on_tab: EventHandler<ExtraTab>,
) -> Element {
    let kanji_visible = !kanji.is_empty();
    let examples_visible = !examples.is_empty();
    if !kanji_visible && !examples_visible {
        return rsx! { Fragment {} };
    }
    // If the requested tab has no content, fall back to whichever does.
    let active = match tab {
        ExtraTab::Kanji if !kanji_visible => ExtraTab::Examples,
        ExtraTab::Examples if !examples_visible => ExtraTab::Kanji,
        t => t,
    };

    rsx! {
        div { class: "expansion-panel",
            div { class: "subtabs",
                if kanji_visible {
                    button {
                        class: if active == ExtraTab::Kanji { "active" } else { "" },
                        onclick: move |_| on_tab.call(ExtraTab::Kanji),
                        "Kanji"
                    }
                }
                if examples_visible {
                    button {
                        class: if active == ExtraTab::Examples { "active" } else { "" },
                        onclick: move |_| on_tab.call(ExtraTab::Examples),
                        "Examples"
                    }
                }
            }
            match active {
                ExtraTab::Kanji => rsx! {
                    div {
                        for k in kanji.iter().cloned() {
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
                ExtraTab::Examples => rsx! {
                    div {
                        for ex in examples.iter().cloned() {
                            div { class: "example-row",
                                div { class: "jp", ExampleJp { sentence: ex.japanese.clone(), word: word.clone() } }
                                div { class: "en", "{ex.english}" }
                            }
                        }
                    }
                },
            }
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
