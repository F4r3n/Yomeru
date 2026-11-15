//! Grammar results on the lookup page.
//!
//! A card per grammar point. Closed, it shows what a glance needs: the
//! headword, its level, a two-line meaning, the structure and two
//! examples. Open, it shows one source at a time behind tabs, starting with
//! the one explaining the most, each with its explanation, then its
//! patterns, then its examples.

use dioxus::prelude::*;

use crate::grammar::{Example, GrammarEntry, ParagraphKind, Rich, SourceBlock, Span};
use crate::icons::{Icon, IconName};

/// Examples a closed card previews.
const PREVIEW_EXAMPLES: usize = 2;

/// The Grammar section: `entries` as cards. `on_search` searches a headword
/// a card links to.
#[component]
pub fn GrammarResults(entries: Vec<GrammarEntry>, on_search: EventHandler<String>) -> Element {
    if entries.is_empty() {
        return rsx! { Fragment {} };
    }
    rsx! {
        div { class: "grammar-results",
            div { class: "section-title", "Grammar" }
            for (i, entry) in entries.into_iter().enumerate() {
                GrammarEntryCard { key: "{i}-{entry.term}", entry, on_search }
            }
        }
    }
}

/// The source's examples, all senses together.
fn examples_of(source: &SourceBlock) -> impl Iterator<Item = &Example> {
    source.senses.iter().flat_map(|s| s.examples())
}

#[component]
fn GrammarEntryCard(entry: GrammarEntry, on_search: EventHandler<String>) -> Element {
    let mut open = use_signal(|| false);
    let main_source = entry.main_source();
    let mut tab = use_signal(move || main_source);
    let is_open = *open.read();
    let has_detail = !entry.sources.is_empty();
    // The first source with examples feeds the preview: some sources are
    // prose only.
    let preview: Vec<Example> = entry
        .sources
        .iter()
        .map(|s| {
            examples_of(s)
                .take(PREVIEW_EXAMPLES)
                .cloned()
                .collect::<Vec<_>>()
        })
        .find(|e| !e.is_empty())
        .unwrap_or_default();
    let active = (*tab.read()).min(entry.sources.len().saturating_sub(1));
    let meaning = entry.full_meaning();

    rsx! {
        div { class: if is_open { "card grammar-card open" } else { "card grammar-card" },
            div {
                class: if has_detail { "grammar-head clickable" } else { "grammar-head" },
                onclick: move |_| if has_detail { open.toggle() },
                div { class: "grammar-title",
                    span { class: "headword", lang: "ja", "{entry.term}" }
                    if !entry.reading.is_empty() && entry.reading != entry.term {
                        span { class: "kanji-sub", lang: "ja", "{entry.reading}" }
                    }
                    if !entry.jlpt.is_empty() {
                        span { class: "freq-badge", "{entry.jlpt}" }
                    }
                    if has_detail {
                        span {
                            class: "grammar-chevron",
                            title: if is_open { "Close" } else { "Open" },
                            Icon { name: IconName::ChevronDown, size: 18 }
                        }
                    }
                }
                if !meaning.is_empty() {
                    div { class: "gloss grammar-meaning", "{meaning}" }
                }
                if !entry.structure.is_empty() {
                    div { class: "grammar-structure", lang: "ja", "{entry.structure}" }
                }
            }
            if !entry.see_also.is_empty() {
                div { class: "grammar-see",
                    span { class: "muted", "See" }
                    for target in entry.see_also.iter().cloned() {
                        button {
                            class: "chip",
                            lang: "ja",
                            onclick: {
                                let target = target.clone();
                                move |_| on_search.call(target.clone())
                            },
                            "{target}"
                        }
                    }
                }
            }
            if !is_open && !preview.is_empty() {
                ExampleList { examples: preview, on_search }
            }
            if is_open {
                if entry.sources.len() > 1 {
                    div { class: "subtabs grammar-tabs",
                        for (i, source) in entry.sources.iter().enumerate() {
                            button {
                                key: "{i}",
                                class: if i == active { "active" } else { "" },
                                onclick: move |_| tab.set(i),
                                "{source.name}"
                            }
                        }
                    }
                }
                if let Some(source) = entry.sources.get(active).cloned() {
                    SourceView { key: "{active}", source, on_search }
                }
                if !entry.website.is_empty() {
                    a { class: "grammar-website", href: "{entry.website}", target: "_blank",
                        "All explanations and illustrations on the dictionary's website ↗"
                    }
                }
            }
        }
    }
}

/// One source: per sense, its explanation, then its patterns, then its
/// examples.
#[component]
fn SourceView(source: SourceBlock, on_search: EventHandler<String>) -> Element {
    rsx! {
        div { class: "grammar-source",
            div { class: "grammar-source-name",
                span { "{source.name}" }
                if !source.level.is_empty() {
                    span { class: "freq-badge", "{source.level}" }
                }
            }
            for (i, sense) in source.senses.iter().enumerate() {
                {
                    // Without the paragraphs that only repeat an example.
                    let prose: Vec<(ParagraphKind, Rich)> =
                        sense.prose().map(|(k, t)| (k, t.clone())).collect();
                    let patterns: Vec<Rich> = sense.patterns().cloned().collect();
                    let examples: Vec<Example> = sense.examples().cloned().collect();
                    rsx! {
                        div { key: "{i}", class: "grammar-sense",
                            if !sense.label.is_empty() {
                                div { class: "grammar-sense-label", "{sense.label}" }
                            }
                            if !prose.is_empty() {
                                div { class: "grammar-prose",
                                    for (j, (kind, text)) in prose.into_iter().enumerate() {
                                        p { key: "{j}", class: paragraph_class(kind),
                                            RichText { rich: text, on_search }
                                        }
                                    }
                                }
                            }
                            if !patterns.is_empty() {
                                div { class: "grammar-patterns", lang: "ja",
                                    for (j, item) in patterns.into_iter().enumerate() {
                                        span { key: "{j}", class: "grammar-pattern",
                                            RichText { rich: item, on_search }
                                        }
                                    }
                                }
                            }
                            if !examples.is_empty() {
                                ExampleList { examples, on_search }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn paragraph_class(kind: ParagraphKind) -> &'static str {
    match kind {
        ParagraphKind::Plain => "grammar-p",
        ParagraphKind::Translation => "grammar-p translation",
        ParagraphKind::Heading => "grammar-p heading",
        ParagraphKind::InlineExample => "grammar-p inline-example",
        ParagraphKind::Derivation => "grammar-p derivation",
        ParagraphKind::Annotation => "grammar-p annotation",
        ParagraphKind::ExampleLabel => "grammar-p example-label",
    }
}

#[component]
fn ExampleList(examples: Vec<Example>, on_search: EventHandler<String>) -> Element {
    rsx! {
        div { class: "grammar-examples",
            for (i, ex) in examples.into_iter().enumerate() {
                div { key: "{i}", class: "grammar-example",
                    div { class: "jp", lang: "ja", RichText { rich: ex.ja, on_search } }
                    if !ex.en.is_empty() {
                        div { class: "en", "{ex.en}" }
                    }
                }
            }
        }
    }
}

#[component]
fn RichText(rich: Rich, on_search: EventHandler<String>) -> Element {
    rsx! {
        for (i, span) in rich.into_iter().enumerate() {
            match span {
                Span::Text { text } => rsx! { span { key: "{i}", "{text}" } },
                Span::Highlight { text } => rsx! { mark { key: "{i}", "{text}" } },
                Span::Label { text } => rsx! { strong { key: "{i}", "{text}" } },
                Span::Ruby { text, ruby } => rsx! {
                    ruby { key: "{i}", "{text}" rt { "{ruby}" } }
                },
                Span::Link { text, query } => rsx! {
                    button {
                        key: "{i}",
                        class: "link grammar-link",
                        onclick: move |_| on_search.call(query.clone()),
                        "{text}"
                    }
                },
                Span::LineBreak => rsx! { br { key: "{i}" } },
            }
        }
    }
}
