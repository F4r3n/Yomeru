//! Grammar results on the lookup page.
//!
//! A card per grammar point. Closed, it shows what a glance needs: the
//! pattern, its level, the meaning, how it attaches and two examples.
//! Open, it adds every construction, the explanation, the notes, all the
//! examples and the points worth comparing.

use dioxus::prelude::*;

use crate::grammar::{Example, GrammarEntry, Span};
use crate::icons::{Icon, IconName};
use crate::idb::{bump_priority, get_cards_by_sequence, put_card, reset_card};
use crate::srs::now_ms;
use crate::sync::schedule_sync;
use crate::types::{CardStatus, MAX_PRIORITY, SrsCard};

/// Examples a closed card previews.
const PREVIEW_EXAMPLES: usize = 2;
/// Other spellings listed before "+N".
const FORMS_SHOWN: usize = 6;

/// The Grammar section: `entries` as cards. `on_search` searches a pattern
/// a card links to.
#[component]
pub fn GrammarResults(entries: Vec<GrammarEntry>, on_search: EventHandler<String>) -> Element {
    if entries.is_empty() {
        return rsx! { Fragment {} };
    }
    rsx! {
        div { class: "grammar-results",
            div { class: "section-title", "Grammar" }
            for entry in entries {
                GrammarEntryCard { key: "{entry.id}", entry, on_search }
            }
        }
    }
}

#[component]
fn GrammarEntryCard(entry: GrammarEntry, on_search: EventHandler<String>) -> Element {
    let mut open = use_signal(|| false);
    let is_open = *open.read();
    let shown_examples = if is_open {
        entry.examples.len()
    } else {
        PREVIEW_EXAMPLES
    };
    let hidden_forms = entry.forms.len().saturating_sub(FORMS_SHOWN);
    let forms = entry
        .forms
        .iter()
        .take(FORMS_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join("、");

    rsx! {
        div { class: if is_open { "card grammar-card open" } else { "card grammar-card" },
            div {
                class: "grammar-head clickable",
                onclick: move |_| open.toggle(),
                div { class: "grammar-title",
                    span { class: "headword", lang: "ja", "{entry.pattern}" }
                    span { class: "freq-badge", "{entry.jlpt}" }
                    span {
                        class: "grammar-chevron",
                        title: if is_open { "Close" } else { "Open" },
                        Icon { name: IconName::ChevronDown, size: 18 }
                    }
                }
                div { class: "gloss grammar-meaning", "{entry.meaning}" }
                GrammarDeckButton { id: entry.id.clone(), seq: entry.seq }
                if !is_open {
                    if let Some(first) = entry.formation.first() {
                        div { class: "grammar-structure", lang: "ja", "{first}" }
                    }
                }
            }
            if is_open {
                if !forms.is_empty() {
                    div { class: "grammar-forms muted", lang: "ja",
                        "Also: {forms}"
                        if hidden_forms > 0 { " +{hidden_forms}" }
                    }
                }
                div { class: "grammar-section",
                    div { class: "grammar-section-title", "Formation" }
                    ul { class: "grammar-patterns", lang: "ja",
                        for (i, line) in entry.formation.iter().enumerate() {
                            li { key: "{i}", "{line}" }
                        }
                    }
                }
                p { class: "grammar-p", "{entry.explanation}" }
                if !entry.notes.is_empty() {
                    div { class: "grammar-section",
                        div { class: "grammar-section-title", "Notes" }
                        p { class: "grammar-p", "{entry.notes}" }
                    }
                }
            }
            if !entry.examples.is_empty() {
                div { class: if is_open { "grammar-section" } else { "" },
                    if is_open {
                        div { class: "grammar-section-title", "Examples" }
                    }
                    ExampleList {
                        examples: entry.examples.iter().take(shown_examples).cloned().collect::<Vec<_>>(),
                    }
                }
            }
            if is_open && !entry.related.is_empty() {
                div { class: "grammar-see",
                    span { class: "muted", "Compare" }
                    for rel in entry.related.iter().cloned() {
                        button {
                            key: "{rel.id}",
                            class: "chip",
                            lang: "ja",
                            onclick: move |_| on_search.call(rel.pattern.clone()),
                            "{rel.pattern}"
                        }
                    }
                }
            }
            if is_open {
                a {
                    class: "grammar-more-link",
                    href: "{bee_search_url(&entry.pattern)}",
                    target: "_blank",
                    rel: "noopener",
                    "More on Bee's Ultimate Grammar Dictionary ↗"
                }
            }
        }
    }
}

/// Bee-san's grammar dictionary site, searched for `pattern`: a search
/// rather than a page, so nothing of theirs is stored here and no id can go
/// stale. No `level`: theirs can differ from ours (Bunpro's).
fn bee_search_url(pattern: &str) -> String {
    const SITE: &str = "https://bee-san.github.io/bees-ultimate-grammar-dictionary/";
    let mut url = format!("{SITE}?q=");
    for b in pattern.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// The point's card in the deck: Add, then the staging priority like a
/// word, then its state. The card is `SrsCard::new_grammar` of the entry's
/// `seq`, so `get_cards_by_sequence` finds exactly that one.
#[component]
fn GrammarDeckButton(id: String, seq: u32) -> Element {
    let sequence = crate::grammar::card_sequence(seq);
    let mut card = use_signal(|| None::<SrsCard>);
    let mut busy = use_signal(|| false);
    use_effect(use_reactive!(|sequence| {
        spawn(async move {
            match get_cards_by_sequence(sequence).await {
                Ok(cards) => card.set(cards.into_iter().next()),
                Err(e) => warn!("get_cards_by_sequence(grammar {sequence}) failed: {e}"),
            }
        });
    }));

    let refresh = move || async move {
        let cards = get_cards_by_sequence(sequence).await.unwrap_or_default();
        card.set(cards.into_iter().next());
    };
    let id_for_add = id.clone();
    let on_add = move |e: MouseEvent| {
        e.stop_propagation();
        if *busy.peek() {
            return;
        }
        busy.set(true);
        let id = id_for_add.clone();
        spawn(async move {
            let current = card.peek().clone();
            let res = match current {
                None => put_card(&SrsCard::new_grammar(seq, now_ms())).await,
                Some(_) => bump_priority(sequence).await,
            };
            match res {
                Ok(()) => schedule_sync(),
                Err(e) => warn!("adding grammar card {id} failed: {e}"),
            }
            refresh().await;
            busy.set(false);
        });
    };
    let on_reset = move |e: MouseEvent| {
        e.stop_propagation();
        spawn(async move {
            let confirmed = crate::rt::confirm(
                "Reset this grammar point's SRS progress? This can't be undone.",
            )
            .await;
            if !confirmed {
                return;
            }
            match reset_card(sequence, now_ms()).await {
                Ok(()) => schedule_sync(),
                Err(e) => warn!("reset_card(grammar {sequence}) failed: {e}"),
            }
            refresh().await;
        });
    };

    let current = card.read().clone();
    let disabled = *busy.read();
    rsx! {
        div { class: "grammar-deck",
            match current {
                None => rsx! {
                    button { class: "primary", disabled, onclick: on_add, "+ Add to deck" }
                },
                Some(c) if c.status == CardStatus::Staging && c.priority >= MAX_PRIORITY => rsx! {
                    button { class: "success", disabled: true, "★ Staged · Priority maxed ({MAX_PRIORITY})" }
                },
                Some(c) if c.status == CardStatus::Staging => rsx! {
                    button { class: "success", disabled, onclick: on_add, "☆ Staged · +Priority (★{c.priority})" }
                },
                Some(c) => rsx! {
                    button { class: "success", onclick: on_reset,
                        if c.status == CardStatus::Graduated { "✓ Graduated · Reset" } else { "✓ In deck · Reset" }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::bee_search_url;

    #[test]
    fn the_bee_link_searches_the_encoded_pattern() {
        assert_eq!(
            bee_search_url("くらい"),
            "https://bee-san.github.io/bees-ultimate-grammar-dictionary/?q=%E3%81%8F%E3%82%89%E3%81%84"
        );
        assert_eq!(
            bee_search_url("a b&c"),
            "https://bee-san.github.io/bees-ultimate-grammar-dictionary/?q=a%20b%26c"
        );
    }
}

/// Everything an open entry holds, for the back of a review card: forms,
/// every construction, the explanation, the notes, all the examples and the
/// points worth comparing. The comparisons are plain text there, so reading
/// them never leaves the session.
#[component]
pub(crate) fn GrammarExplanation(entry: GrammarEntry) -> Element {
    let forms = entry.forms.join("、");
    let related = entry
        .related
        .iter()
        .map(|r| r.pattern.as_str())
        .collect::<Vec<_>>()
        .join("、");
    rsx! {
        div { class: "grammar-full",
            if !forms.is_empty() {
                div { class: "grammar-forms muted", lang: "ja", "Also: {forms}" }
            }
            div { class: "grammar-section",
                div { class: "grammar-section-title", "Formation" }
                ul { class: "grammar-patterns", lang: "ja",
                    for (i, line) in entry.formation.iter().enumerate() {
                        li { key: "{i}", "{line}" }
                    }
                }
            }
            p { class: "grammar-p", "{entry.explanation}" }
            if !entry.notes.is_empty() {
                div { class: "grammar-section",
                    div { class: "grammar-section-title", "Notes" }
                    p { class: "grammar-p", "{entry.notes}" }
                }
            }
            if !entry.examples.is_empty() {
                div { class: "grammar-section",
                    div { class: "grammar-section-title", "Examples" }
                    ExampleList { examples: entry.examples.clone() }
                }
            }
            if !related.is_empty() {
                div { class: "grammar-see",
                    span { class: "muted", "Compare" }
                    span { lang: "ja", "{related}" }
                }
            }
        }
    }
}

#[component]
pub(crate) fn ExampleList(examples: Vec<Example>) -> Element {
    rsx! {
        div { class: "grammar-examples",
            for (i, ex) in examples.into_iter().enumerate() {
                div { key: "{i}", class: "grammar-example",
                    div { class: "jp", lang: "ja",
                        for (j, span) in ex.ja.iter().cloned().enumerate() {
                            match span {
                                Span::Text { text } => rsx! { span { key: "{j}", "{text}" } },
                                Span::Highlight { text } => rsx! { mark { key: "{j}", "{text}" } },
                            }
                        }
                    }
                    div { class: "en", "{ex.en}" }
                }
            }
        }
    }
}
