//! Grammar results on the lookup page.
//!
//! A card per grammar point. Closed, it shows what a glance needs: the
//! pattern, its level, the meaning, how it attaches and two examples.
//! Open, it adds every construction, the explanation, the notes, all the
//! examples and the points worth comparing.

use dioxus::prelude::*;

use crate::grammar::{Example, GrammarEntry, Span};
use crate::icons::{Icon, IconName};

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

#[component]
fn ExampleList(examples: Vec<Example>) -> Element {
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
