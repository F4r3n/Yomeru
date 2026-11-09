//! Find a kanji you can't type: pick the parts you see, tap the kanji, and
//! the word builds up in a tray that goes to the word search.
//!
//! The state lives in global signals so it survives a trip to the word
//! search and back: the tray still holds 清 when you come back for 潔.

use dioxus::prelude::*;
use kanjidic_types::{PartInfo, PartSearch};

use crate::app::Route;
use crate::dict;
use crate::icons::{Icon, IconName};

/// Every part, loaded once per app run.
static PARTS: GlobalSignal<Vec<PartInfo>> = Signal::global(Vec::new);
/// The selected parts, in the order they were picked.
static SELECTED: GlobalSignal<Vec<char>> = Signal::global(Vec::new);
/// The word being built from the tapped kanji.
static TRAY: GlobalSignal<String> = Signal::global(String::new);

/// The grid leaves out parts in fewer kanji than this: rare shapes that
/// would bury the useful ones.
const GRID_MIN_KANJI: u32 = 15;
/// How many matches to fetch; past this, another part narrows it down.
const MAX_RESULTS: u8 = 100;

/// Words / Kanji switch at the top of both lookup modes.
#[component]
pub fn LookupModeTabs(kanji: bool) -> Element {
    rsx! {
        div { class: "seg-tabs lookup-modes",
            Link { to: Route::Lookup {}, class: if kanji { "" } else { "active" }, "Words" }
            Link { to: Route::KanjiFinder {}, class: if kanji { "active" } else { "" }, "Find a kanji" }
        }
    }
}

fn stroke_label(n: u8) -> String {
    match n {
        0 => "Other".to_string(),
        1 => "1 stroke".to_string(),
        n => format!("{n} strokes"),
    }
}

/// Where the matches for the current selection stand.
enum Matches {
    /// Asked, or answered for a selection that has since changed.
    Pending,
    Failed(String),
    Ready(PartSearch),
}

#[component]
pub fn KanjiFinder() -> Element {
    let nav = use_navigator();
    let mut load_err = use_signal(|| Option::<String>::None);

    use_future(move || async move {
        if !PARTS.read().is_empty() {
            return;
        }
        match dict::kanji_parts().await {
            Ok(p) => *PARTS.write() = p,
            Err(e) => load_err.set(Some(e)),
        }
    });

    // Each answer carries the selection it was asked for, so one that
    // arrives for an older selection is never shown as the current one.
    let mut search = use_resource(move || {
        let selected: String = SELECTED.read().iter().collect();
        async move {
            let answer = if selected.is_empty() {
                Ok(None)
            } else {
                dict::kanji_with_parts(&selected, MAX_RESULTS)
                    .await
                    .map(Some)
            };
            (selected, answer)
        }
    });

    let current: String = SELECTED.read().iter().collect();
    let matches = match &*search.read() {
        Some((asked, answer)) if *asked == current => match answer {
            Ok(Some(f)) => Matches::Ready(f.clone()),
            Ok(None) => Matches::Pending,
            Err(e) => Matches::Failed(e.clone()),
        },
        _ => Matches::Pending,
    };
    let found: Option<&PartSearch> = match &matches {
        Matches::Ready(f) => Some(f),
        _ => None,
    };

    let toggle_part = move |p: char| {
        let mut sel = SELECTED.write();
        if let Some(i) = sel.iter().position(|&c| c == p) {
            sel.remove(i);
        } else {
            sel.push(p);
        }
    };

    // A picked kanji goes in the word, and the parts are cleared for the next.
    let pick_kanji = move |k: char| {
        TRAY.write().push(k);
        SELECTED.write().clear();
    };

    let search_word = move |_| {
        let word = TRAY.read().trim().to_string();
        if word.is_empty() {
            return;
        }
        crate::routes::lookup::search_on_open(word);
        nav.push(Route::Lookup {});
    };

    let selected = SELECTED.read().clone();
    let word: Vec<char> = TRAY.read().chars().collect();
    let parts = PARTS.read();

    // Once something is selected, parts that would leave nothing go grey.
    let open: Option<Vec<char>> = found.map(|f| f.open_parts.clone());
    let tile = |c: char| {
        let is_selected = selected.contains(&c);
        let dead = !is_selected && open.as_ref().is_some_and(|o| o.binary_search(&c).is_err());
        (is_selected, dead)
    };

    let mut rows: Vec<(u8, Vec<&PartInfo>)> = Vec::new();
    for p in parts.iter().filter(|p| p.kanji_count >= GRID_MIN_KANJI) {
        match rows.last_mut() {
            Some((n, row)) if *n == p.strokes => row.push(p),
            _ => rows.push((p.strokes, vec![p])),
        }
    }

    let part_tile = |p: &PartInfo| {
        let c = p.element;
        let (is_selected, dead) = tile(c);
        let title = p.names.join(", ");
        rsx! {
            button {
                class: if is_selected { "part-btn selected" } else { "part-btn" },
                disabled: dead,
                title: "{title}",
                onclick: move |_| toggle_part(c),
                "{c}"
            }
        }
    };

    rsx! {
        div { class: "kanji-finder",
            LookupModeTabs { kanji: true }

            if let Some(err) = load_err.read().clone() {
                div { class: "card error", "Couldn't load the parts: {err}" }
            }

            div { class: "finder-word",
                div { class: "word-slots",
                    if word.is_empty() {
                        span { class: "slot empty", "?" }
                    }
                    for c in word.iter() {
                        span { class: "slot", "{c}" }
                    }
                }
                div { class: "word-actions",
                    button {
                        class: "icon-btn",
                        title: "Remove the last character",
                        "aria-label": "Remove the last character",
                        disabled: word.is_empty(),
                        onclick: move |_| {
                            TRAY.write().pop();
                        },
                        "⌫"
                    }
                    button { class: "primary", disabled: word.is_empty(), onclick: search_word,
                        Icon { name: IconName::Search, size: 16 }
                        "Search"
                    }
                }
            }

            if !selected.is_empty() {
                div { class: "finder-matches",
                    div { class: "matches-head",
                        for &c in selected.iter() {
                            button {
                                class: "part-btn selected small",
                                title: "Remove this part",
                                onclick: move |_| toggle_part(c),
                                "{c}"
                            }
                        }
                        span { class: "muted count",
                            match &matches {
                                Matches::Pending => "…".to_string(),
                                Matches::Failed(_) => String::new(),
                                Matches::Ready(f) if f.total == 0 => "No kanji has all these parts".to_string(),
                                Matches::Ready(f) if f.total as usize > f.kanji.len() => format!("{} of {}: add a part to narrow", f.kanji.len(), f.total),
                                Matches::Ready(f) => format!("{} kanji", f.total),
                            }
                        }
                        button { class: "link clear", onclick: move |_| SELECTED.write().clear(), "Clear" }
                    }
                    if let Matches::Failed(e) = &matches {
                        div { class: "matches-error",
                            span { "Couldn't search: {e}" }
                            button { class: "link", onclick: move |_| search.restart(), "Retry" }
                        }
                    }
                    if let Some(f) = found {
                        if !f.kanji.is_empty() {
                            div { class: "kanji-results",
                                for k in f.kanji.iter() {
                                    {
                                        let lit = k.literal;
                                        let tip = k.meanings.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                                        rsx! {
                                            button {
                                                class: "kanji-hit",
                                                title: "{tip}",
                                                onclick: move |_| pick_kanji(lit),
                                                "{lit}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if parts.is_empty() && load_err.read().is_none() {
                div { class: "loading", "Loading parts…" }
            } else {
                div { class: "all-parts",
                    for (n, row) in rows.iter() {
                        div { class: "stroke-head", "{stroke_label(*n)}" }
                        div { class: "part-tiles",
                            for p in row.iter() {
                                {part_tile(p)}
                            }
                        }
                    }
                }
            }
        }
    }
}
