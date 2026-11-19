//! The Lookup page's Sentence tab: a sentence read into its words and the
//! grammar they make up (`sentence_core`, through
//! [`crate::platform::DictClient::sentence_breakdown`]).
//!
//! A word shows its dictionary form and how it is conjugated; tapping it
//! opens its entry card under the sentence, to add it to the deck or open
//! it in the word lookup. A word that is a grammar point of its own (a
//! particle: は, に) opens that point's card instead. Each
//! grammar point lists under the sentence, its text marked in it while
//! selected.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::EntryCard;
use crate::dict::{self, primary_headword};
use crate::icons::{Icon, IconName};
use crate::idb::{get_cards_of, reset_card};
use crate::platform::{BreakdownGrammar, BreakdownWord, Platform, SentenceBreakdown};
use crate::routes::kanji_finder::{LookupMode, LookupModeTabs};
use crate::routes::lookup::search_on_open;
use crate::routes::lookup::{add_or_bump, status_of};
use crate::routes::lookup_grammar::GrammarEntryCard;
use crate::srs::now_ms;
use crate::sync::schedule_sync;
use crate::types::{CardItem, CardStatus};

/// The sentence last read, kept while the user opens a word and comes
/// back. A global signal, not a thread-local: on Android, Dioxus runs the
/// app from more than one thread.
static SENTENCE: GlobalSignal<String> = Signal::global(String::new);
/// Its breakdown, kept with it.
static BREAKDOWN: GlobalSignal<Option<SentenceBreakdown>> = Signal::global(|| None);

/// Longest sentence read, in chars (the server's limit on a query).
const MAX_CHARS: usize = 256;

/// Whether the platform reads sentences (not yet the extension).
pub fn available() -> bool {
    consume_context::<Platform>().dict.has_sentence_breakdown()
}

#[component]
pub fn SentenceBreakdownPage() -> Element {
    let nav = use_navigator();
    let mut error = use_signal(|| Option::<String>::None);
    let mut reading = use_signal(|| false);
    // The grammar point selected, by its place in the breakdown's list.
    let mut selected = use_signal(|| Option::<usize>::None);
    // The word whose card is open, by its place in the breakdown's words.
    let mut word = use_signal(|| Option::<usize>::None);
    // The deck state of the words opened, keyed by JMdict sequence, as on
    // the word lookup.
    let mut card_status = use_signal(HashMap::<u32, CardStatus>::new);
    let card_priority = use_signal(HashMap::<u32, u32>::new);
    let mut locked = use_signal(HashSet::<u32>::new);

    // Read once typing pauses. `use_resource` drops the previous future
    // at its sleep when the text changes, so only the last one asks.
    let _reader = use_resource(move || {
        let text = SENTENCE.read().trim().to_owned();
        async move {
            if text.is_empty() {
                *BREAKDOWN.write() = None;
                return;
            }
            crate::rt::sleep_ms(300).await;
            reading.set(true);
            let text: String = text.chars().take(MAX_CHARS).collect();
            match dict::sentence_breakdown(&text).await {
                Ok(b) => {
                    error.set(None);
                    selected.set(None);
                    word.set(None);
                    *BREAKDOWN.write() = Some(b);
                }
                Err(e) => error.set(Some(e)),
            }
            reading.set(false);
        }
    });

    let text = SENTENCE.read().trim().to_owned();
    let chars: Vec<char> = text.chars().collect();
    let breakdown = BREAKDOWN.read().clone();
    let sel = *selected.read();

    // Opens word `i`'s card (closes it if open), with its deck state.
    let mut open_word = move |i: usize, w: BreakdownWord| {
        if *word.read() == Some(i) {
            word.set(None);
            return;
        }
        let Some(entry) = w.entry else {
            return;
        };
        word.set(Some(i));
        let seq = entry.sequence;
        let mut card_priority = card_priority;
        spawn(async move {
            let cards = get_cards_of(CardItem::Word(seq)).await.unwrap_or_default();
            card_status.with_mut(|m| match status_of(&cards) {
                Some(s) => {
                    m.insert(seq, s);
                }
                None => {
                    m.remove(&seq);
                }
            });
            if let Some(p) = cards.iter().map(|c| c.priority).max() {
                card_priority.with_mut(|m| {
                    m.insert(seq, p);
                });
            }
        });
    };
    let on_add = move |seq: u32| {
        locked.with_mut(|s| {
            s.insert(seq);
        });
        spawn(async move { add_or_bump(seq, card_status, card_priority).await });
    };
    let on_reset = move |seq: u32| {
        spawn(async move {
            if !crate::rt::confirm("Reset this word's SRS progress? This can't be undone.").await {
                return;
            }
            if let Err(e) = reset_card(CardItem::Word(seq), now_ms()).await {
                warn!("reset_card(seq={seq}) failed: {e}");
                return;
            }
            schedule_sync();
            card_status.with_mut(|m| {
                m.insert(seq, CardStatus::Active);
            });
        });
    };
    // The open word, its text, and its entry.
    let open_entry = breakdown.as_ref().zip(*word.read()).and_then(|(b, i)| {
        let w = b.words.get(i)?;
        let surface: String = chars.iter().skip(w.start).take(w.len).collect();
        Some((w.clone(), surface, w.entry.clone()?))
    });

    rsx! {
        div {
            LookupModeTabs { mode: LookupMode::Sentence }
            if let Some(err) = error.read().clone() {
                div { class: "card error", "Couldn't read the sentence: {err}" }
            }
            div { class: "hero-search", style: "margin-bottom: 14px;",
                textarea {
                    class: "sentence-input",
                    lang: "ja",
                    rows: 2,
                    placeholder: "Paste a Japanese sentence…",
                    autofocus: true,
                    value: "{SENTENCE}",
                    oninput: move |e| *SENTENCE.write() = e.value(),
                }
            }
            match breakdown {
                Some(b) if !text.is_empty() => rsx! {
                    Words {
                        chars: chars.clone(),
                        words: b.words.clone(),
                        grammar: b.grammar.clone(),
                        selected: sel,
                        open_word: *word.read(),
                        on_word: EventHandler::new(move |i: usize| {
                            let Some(w) = BREAKDOWN.read().as_ref().and_then(|b| b.words.get(i).cloned()) else {
                                return;
                            };
                            // A word that is a point of its own opens it.
                            let own = BREAKDOWN.read().as_ref().and_then(|b| {
                                b.grammar.iter().position(|m| (m.start, m.len) == (w.start, w.len))
                            });
                            match own {
                                Some(g) => {
                                    word.set(None);
                                    select(selected, g);
                                }
                                None => {
                                    // One selection at a time: the word's
                                    // card, or a point's.
                                    selected.set(None);
                                    open_word(i, w);
                                }
                            }
                        }),
                    }
                    if let Some((w, surface, entry)) = open_entry {
                        {
                            let seq = entry.sequence;
                            let head = primary_headword(&entry).to_owned();
                            // How the word stands in the sentence: 言われ →
                            // 言う · passive · stem.
                            let how: Vec<String> = w.reasons.iter().map(|r| reason_label(r)).collect();
                            let conjugated = w.base != surface || !how.is_empty();
                            rsx! {
                                div { class: "sentence-entry",
                                    if conjugated {
                                        div { class: "sentence-form",
                                            span { lang: "ja", "{surface}" }
                                            span { class: "muted", " → " }
                                            span { lang: "ja", "{w.base}" }
                                            if !how.is_empty() {
                                                span { class: "sentence-how", {how.join(" · ")} }
                                            }
                                        }
                                    }
                                    EntryCard {
                                        key: "{seq}",
                                        entry,
                                        on_add: Some(EventHandler::new(on_add)),
                                        on_reset: Some(EventHandler::new(on_reset)),
                                        on_select: None,
                                        status: card_status.read().get(&seq).copied(),
                                        priority: card_priority.read().get(&seq).copied().unwrap_or(0),
                                        locked: locked.read().contains(&seq),
                                    }
                                    div { class: "row sentence-entry-actions",
                                        button {
                                            onclick: move |_| {
                                                search_on_open(head.clone());
                                                nav.push(Route::Lookup {});
                                            },
                                            "Open in word search"
                                        }
                                        button { onclick: move |_| word.set(None), "Close" }
                                    }
                                }
                            }
                        }
                    }
                    GrammarList {
                        chars,
                        grammar: b.grammar,
                        selected: sel,
                        on_select: EventHandler::new(move |g: usize| {
                            if *selected.read() == Some(g) {
                                selected.set(None);
                            } else {
                                word.set(None);
                                select(selected, g);
                            }
                        }),
                    }
                },
                _ if *reading.read() => rsx! { div { class: "loading", "Reading…" } },
                _ => rsx! {
                    div { class: "empty-state",
                        div { class: "glyph", "文" }
                        div { class: "headline", "Read a sentence" }
                        div { class: "helper", "Its words, their dictionary forms, and the grammar they make up. e.g. 彼女は買い物に行った。" }
                    }
                },
            }
        }
    }
}

/// Selects grammar point `g` and brings its card into view.
fn select(mut selected: Signal<Option<usize>>, g: usize) {
    selected.set(Some(g));
    document::eval(&format!(
        "requestAnimationFrame(() => document.getElementById('sentence-grammar-{g}')\
         ?.scrollIntoView({{behavior: 'smooth', block: 'nearest'}}));"
    ));
}

/// What a conjugation reason reads as under a word: `te-form` → "te form".
fn reason_label(name: &str) -> String {
    match name {
        "desiderative" => "たい".to_owned(),
        "progressive" => "ている".to_owned(),
        "masu-stem" => "stem".to_owned(),
        "suru-stem" => "する".to_owned(),
        "adverbial" => "く form".to_owned(),
        "nominalization" => "さ noun".to_owned(),
        other => other.replace('-', " "),
    }
}

/// The sentence as written, each word underlined (a word that is a grammar
/// point of its own, a particle, in another colour), the text between them
/// (digits, punctuation) as it is. The selected grammar point's text and
/// the open word are marked.
#[component]
fn Words(
    chars: Vec<char>,
    words: Vec<BreakdownWord>,
    grammar: Vec<BreakdownGrammar>,
    selected: Option<usize>,
    /// The word whose card is open, marked.
    open_word: Option<usize>,
    on_word: EventHandler<usize>,
) -> Element {
    let marked = |start: usize, len: usize| -> bool {
        selected.and_then(|g| grammar.get(g)).is_some_and(|m| {
            m.pieces
                .iter()
                .any(|&(s, n)| start < s + n && s < start + len)
        })
    };
    let own_point =
        |w: &BreakdownWord| grammar.iter().any(|m| (m.start, m.len) == (w.start, w.len));
    let cut = |start: usize, len: usize| -> String { chars.iter().skip(start).take(len).collect() };
    // Each word, and the uncovered text before it.
    let mut parts: Vec<(Option<usize>, usize, usize)> = Vec::new();
    let mut at = 0;
    for (i, w) in words.iter().enumerate() {
        if w.start > at {
            parts.push((None, at, w.start - at));
        }
        parts.push((Some(i), w.start, w.len));
        at = w.start + w.len;
    }
    if at < chars.len() {
        parts.push((None, at, chars.len() - at));
    }

    rsx! {
        div { class: "card sentence-text", lang: "ja",
            for (i, start, len) in parts {
                match i.and_then(|i| words.get(i).map(|w| (i, w))) {
                    Some((i, w)) => {
                        let mut class = String::from("sentence-word");
                        if w.entry.is_none() {
                            class.push_str(" unknown");
                        } else if own_point(w) {
                            class.push_str(" point");
                        }
                        if marked(start, len) {
                            class.push_str(" marked");
                        }
                        if open_word == Some(i) {
                            class.push_str(" open");
                        }
                        rsx! {
                            span {
                                key: "w{i}",
                                class,
                                role: "button",
                                title: w.entry.as_ref().map(dict::short_gloss).unwrap_or_default(),
                                onclick: move |_| on_word.call(i),
                                {cut(start, len)}
                            }
                        }
                    }
                    None => rsx! {
                        span { key: "t{start}", {cut(start, len)} }
                    },
                }
            }
        }
    }
}

/// The grammar points found, in sentence order, one row each: the text it
/// covers and each point's pattern, level and meaning. Only the selected
/// row shows its cards.
#[component]
fn GrammarList(
    chars: Vec<char>,
    grammar: Vec<BreakdownGrammar>,
    selected: Option<usize>,
    on_select: EventHandler<usize>,
) -> Element {
    let nav = use_navigator();
    if grammar.is_empty() {
        return rsx! {
            div { class: "muted", style: "font-size: 13px;", "No grammar point found in this sentence." }
        };
    }
    let on_search = EventHandler::new(move |term: String| {
        search_on_open(term);
        nav.push(Route::Lookup {});
    });
    rsx! {
        div { class: "grammar-results",
            div { class: "section-title", "Grammar" }
            for (g, m) in grammar.into_iter().enumerate() {
                {
                    let text = m
                        .pieces
                        .iter()
                        .map(|&(s, n)| chars.iter().skip(s).take(n).collect::<String>())
                        .collect::<Vec<_>>()
                        .join("〜");
                    let open = selected == Some(g);
                    rsx! {
                        div {
                            key: "{g}",
                            id: "sentence-grammar-{g}",
                            class: if open { "sentence-grammar selected" } else { "sentence-grammar" },
                            button {
                                class: "sentence-grammar-row",
                                title: if open { "Close" } else { "Open" },
                                onclick: move |_| on_select.call(g),
                                span { class: "sentence-grammar-text", lang: "ja", "{text}" }
                                span { class: "sentence-grammar-points",
                                    for entry in m.entries.iter() {
                                        span { key: "{entry.id}", class: "sentence-grammar-point",
                                            span { class: "pattern", lang: "ja", "{entry.pattern}" }
                                            span { class: "freq-badge", "{entry.jlpt}" }
                                            span { class: "meaning", "{entry.meaning}" }
                                        }
                                    }
                                }
                                span { class: if open { "grammar-chevron open" } else { "grammar-chevron" },
                                    Icon { name: IconName::ChevronDown, size: 18 }
                                }
                            }
                            if open {
                                for entry in m.entries {
                                    GrammarEntryCard { key: "{entry.id}", entry, on_search }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
