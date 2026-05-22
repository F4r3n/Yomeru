use std::collections::HashSet;

use dioxus::prelude::*;
use gloo_storage::{LocalStorage, Storage};
use jmdict_types::WordEntry;
use log::warn;

use crate::app::Route;
use crate::components::EntryCard;
use crate::dict::{self, examples_for, kanji_for, primary_headword};
use crate::idb::{get_cards_by_word, put_cards};
use crate::srs::now_ms;
use crate::sync::schedule_sync;
use crate::types::{CardDirection, SrsCard};

const HISTORY_KEY: &str = "lookup_history";
const HISTORY_MAX: usize = 10;

#[derive(Clone, Copy, PartialEq)]
enum ExtraTab {
    Kanji,
    Examples,
}

/// Shared lookup state across the list and the (currently empty) child
/// route components. Currently just `added` so `+ Add` from the expansion
/// panel can flip the badge on the result card too.
#[derive(Clone, Copy)]
struct LookupShared {
    added: Signal<HashSet<String>>,
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

fn is_romaji(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
}

/// Hepburn romaji → hiragana. Mirrors `extension/src/options/romaji.ts`.
fn romaji_to_hiragana(input: &str) -> String {
    let s = input.to_lowercase();
    let bytes: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != 'n'
            && c.is_ascii_alphabetic()
            && !matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
            && i + 1 < bytes.len()
            && bytes[i + 1] == c
        {
            out.push('っ');
            i += 1;
            continue;
        }
        if c == 'n'
            && (i + 1 >= bytes.len()
                || !matches!(bytes[i + 1], 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
        {
            out.push('ん');
            i += 1;
            continue;
        }
        let mut matched = false;
        for len in [3usize, 2, 1] {
            if i + len > bytes.len() {
                continue;
            }
            let chunk: String = bytes[i..i + len].iter().collect();
            if let Some(rep) = lookup_romaji(&chunk) {
                out.push_str(rep);
                i += len;
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn lookup_romaji(c: &str) -> Option<&'static str> {
    Some(match c {
        "kya" => "きゃ", "kyu" => "きゅ", "kyo" => "きょ",
        "sha" => "しゃ", "shu" => "しゅ", "sho" => "しょ", "shi" => "し",
        "cha" => "ちゃ", "chu" => "ちゅ", "cho" => "ちょ", "chi" => "ち", "tsu" => "つ",
        "nya" => "にゃ", "nyu" => "にゅ", "nyo" => "にょ",
        "hya" => "ひゃ", "hyu" => "ひゅ", "hyo" => "ひょ",
        "mya" => "みゃ", "myu" => "みゅ", "myo" => "みょ",
        "rya" => "りゃ", "ryu" => "りゅ", "ryo" => "りょ",
        "gya" => "ぎゃ", "gyu" => "ぎゅ", "gyo" => "ぎょ",
        "ja" => "じゃ", "ju" => "じゅ", "jo" => "じょ", "ji" => "じ",
        "jya" => "じゃ", "jyu" => "じゅ", "jyo" => "じょ",
        "bya" => "びゃ", "byu" => "びゅ", "byo" => "びょ",
        "pya" => "ぴゃ", "pyu" => "ぴゅ", "pyo" => "ぴょ",
        "ka" => "か", "ki" => "き", "ku" => "く", "ke" => "け", "ko" => "こ",
        "ga" => "が", "gi" => "ぎ", "gu" => "ぐ", "ge" => "げ", "go" => "ご",
        "sa" => "さ", "su" => "す", "se" => "せ", "so" => "そ",
        "za" => "ざ", "zu" => "ず", "ze" => "ぜ", "zo" => "ぞ",
        "ta" => "た", "te" => "て", "to" => "と",
        "da" => "だ", "de" => "で", "do" => "ど",
        "na" => "な", "ni" => "に", "nu" => "ぬ", "ne" => "ね", "no" => "の",
        "ha" => "は", "hi" => "ひ", "fu" => "ふ", "he" => "へ", "ho" => "ほ",
        "ba" => "ば", "bi" => "び", "bu" => "ぶ", "be" => "べ", "bo" => "ぼ",
        "pa" => "ぱ", "pi" => "ぴ", "pu" => "ぷ", "pe" => "ぺ", "po" => "ぽ",
        "ma" => "ま", "mi" => "み", "mu" => "む", "me" => "め", "mo" => "も",
        "ya" => "や", "yu" => "ゆ", "yo" => "よ",
        "ra" => "ら", "ri" => "り", "ru" => "る", "re" => "れ", "ro" => "ろ",
        "wa" => "わ", "wo" => "を",
        "a" => "あ", "i" => "い", "u" => "う", "e" => "え", "o" => "お",
        "n" => "ん",
        _ => return None,
    })
}

async fn add_word(word: String, mut added: Signal<HashSet<String>>) {
    let existing = match get_cards_by_word(&word).await {
        Ok(e) => e,
        Err(e) => {
            warn!("get_cards_by_word({word}) failed: {e}");
            return;
        }
    };
    if existing.is_empty() {
        let now = now_ms();
        let cards = vec![
            SrsCard::new(&word, CardDirection::Recognition, now),
            SrsCard::new(&word, CardDirection::Recall, now),
        ];
        if let Err(e) = put_cards(&cards).await {
            warn!("put_cards({word}) on add failed: {e}");
            return;
        }
        schedule_sync();
    }
    added.with_mut(|s| {
        s.insert(word);
    });
}

// ── Layout (shared by /lookup and /lookup/:word) ─────────────────────
//
// Both routes render the same single-column list. The URL just tells the
// list which card to expand inline. Outlet renders nothing (child routes
// are no-ops) but it's wired up so the routes resolve correctly.

#[component]
pub fn LookupLayout() -> Element {
    let added = use_signal(HashSet::<String>::new);
    use_context_provider(|| LookupShared { added });
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
    let LookupShared { mut added } = use_context::<LookupShared>();
    let nav = use_navigator();
    let current = use_route::<Route>();
    let selected_word: Option<String> = match current.clone() {
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

    // Inline-expansion state — kanji + examples for the URL's selected word.
    let mut kanji_data = use_signal(Vec::<kanjidic_types::KanjiEntry>::new);
    let mut examples_data = use_signal(Vec::<examples_types::ExampleEntry>::new);
    let mut extra_tab = use_signal(|| ExtraTab::Kanji);
    let mut last_fetched = use_signal(String::new);

    // Re-fetch kanji/examples when the URL's word changes.
    if let Some(w) = selected_word.clone() {
        if *last_fetched.read() != w {
            last_fetched.set(w.clone());
            extra_tab.set(ExtraTab::Kanji);
            kanji_data.set(Vec::new());
            examples_data.set(Vec::new());
            spawn(async move {
                kanji_data.set(kanji_for(&w).await.unwrap_or_default());
                examples_data.set(examples_for(&w, 5).await.unwrap_or_default());
            });
        }
    } else if !last_fetched.read().is_empty() {
        last_fetched.set(String::new());
    }

    let run_lookup = move |q: String| {
        let q = q.trim().to_string();
        // A new search → collapse whatever card is currently open.
        if on_detail_at_render {
            nav.replace(Route::Lookup {});
        }
        if q.is_empty() {
            entries.set(Vec::new());
            searched.set(false);
            return;
        }
        let target = if is_romaji(&q) { romaji_to_hiragana(&q) } else { q.clone() };
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
            let mut already: HashSet<String> = HashSet::new();
            for e in &results {
                let head = primary_headword(e).to_string();
                if get_cards_by_word(&head)
                    .await
                    .map(|v| !v.is_empty())
                    .unwrap_or(false)
                {
                    already.insert(head);
                }
            }
            added.set(already);
            let single_word = if results.len() == 1 {
                Some(primary_headword(&results[0]).to_string())
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

    let on_input = {
        let mut query = query.clone();
        let mut run_lookup = run_lookup.clone();
        move |evt: Event<FormData>| {
            let v = evt.value();
            query.set(v.clone());
            run_lookup(v);
        }
    };

    let on_history = {
        let mut query = query.clone();
        let mut run_lookup = run_lookup.clone();
        move |term: String| {
            query.set(term.clone());
            run_lookup(term);
        }
    };

    let on_clear_history = move |_| {
        let _ = LocalStorage::delete(HISTORY_KEY);
        history.set(Vec::new());
    };

    let on_add = move |word: String| {
        spawn(async move { add_word(word, added).await });
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
                for entry in entries.read().iter().cloned() {
                    {
                        let head = primary_headword(&entry).to_string();
                        let is_added = added.read().contains(&head);
                        let expanded = selected_word.as_deref() == Some(&head);
                        let nav = nav;
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
                                entry,
                                on_add: Some(EventHandler::new(on_add.clone())),
                                on_select,
                                is_added,
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
                                        onclick: move |_| (on_history.clone())(t.clone()),
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
