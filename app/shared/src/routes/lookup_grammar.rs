//! Grammar results on the lookup page: one compact card per grammar point
//! (headword, level, meaning, structure), opening onto what each source
//! says.

use dioxus::prelude::*;

use crate::grammar::{Block, GrammarEntry, ParagraphKind, Rich, SourceBlock, Span};

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

#[component]
fn GrammarEntryCard(entry: GrammarEntry, on_search: EventHandler<String>) -> Element {
    let mut open = use_signal(|| false);
    let is_open = *open.read();
    let has_detail = !entry.sources.is_empty();

    rsx! {
        div { class: "card grammar-card",
            div {
                class: if has_detail { "grammar-head clickable" } else { "grammar-head" },
                onclick: move |_| if has_detail { open.toggle() },
                div { class: "row", style: "gap: 8px; align-items: baseline; flex-wrap: wrap;",
                    span { class: "headword", lang: "ja", "{entry.term}" }
                    if !entry.reading.is_empty() && entry.reading != entry.term {
                        span { class: "kanji-sub", lang: "ja", "{entry.reading}" }
                    }
                    if !entry.jlpt.is_empty() {
                        span { class: "freq-badge", "{entry.jlpt}" }
                    }
                    if has_detail {
                        span { class: "grammar-toggle muted",
                            if is_open { "Hide" } else { "{entry.sources.len()} source" }
                            if !is_open && entry.sources.len() > 1 { "s" }
                        }
                    }
                }
                if !entry.meaning.is_empty() {
                    div { class: "gloss", "{entry.meaning}" }
                }
                if !entry.structure.is_empty() {
                    div { class: "grammar-structure", lang: "ja", "{entry.structure}" }
                }
            }
            if !entry.see_also.is_empty() {
                div { class: "row", style: "gap: 6px; flex-wrap: wrap; margin-top: 6px;",
                    span { class: "muted", style: "font-size: 12px;", "See" }
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
            if is_open {
                for (i, source) in entry.sources.iter().cloned().enumerate() {
                    SourceView { key: "{i}", source, first: i == 0, on_search }
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

/// One source's explanation; the first opens with the card.
#[component]
fn SourceView(source: SourceBlock, first: bool, on_search: EventHandler<String>) -> Element {
    rsx! {
        details { class: "grammar-source", open: first,
            summary {
                span { "{source.name}" }
                if !source.level.is_empty() {
                    span { class: "freq-badge", "{source.level}" }
                }
            }
            for (i, sense) in source.senses.iter().enumerate() {
                div { key: "{i}", class: "grammar-sense",
                    if !sense.label.is_empty() {
                        div { class: "grammar-sense-label", "{sense.label}" }
                    }
                    for (j, block) in sense.blocks.iter().cloned().enumerate() {
                        BlockView { key: "{j}", block, on_search }
                    }
                }
            }
        }
    }
}

#[component]
fn BlockView(block: Block, on_search: EventHandler<String>) -> Element {
    match block {
        Block::Paragraph { kind, text } => {
            let class = match kind {
                ParagraphKind::Plain => "grammar-p",
                ParagraphKind::Translation => "grammar-p translation",
                ParagraphKind::Heading => "grammar-p heading",
                ParagraphKind::InlineExample => "grammar-p inline-example",
                ParagraphKind::Derivation => "grammar-p derivation",
                ParagraphKind::Annotation => "grammar-p annotation",
                ParagraphKind::ExampleLabel => "grammar-p example-label",
            };
            rsx! {
                p { class, RichText { rich: text, on_search } }
            }
        }
        Block::Patterns { items } => rsx! {
            ul { class: "grammar-patterns", lang: "ja",
                for (i, item) in items.into_iter().enumerate() {
                    li { key: "{i}", RichText { rich: item, on_search } }
                }
            }
        },
        Block::Examples { items } => rsx! {
            div { class: "grammar-examples",
                for (i, ex) in items.into_iter().enumerate() {
                    div { key: "{i}", class: "example-row",
                        div { class: "jp", lang: "ja", RichText { rich: ex.ja, on_search } }
                        if !ex.en.is_empty() {
                            div { class: "en", "{ex.en}" }
                        }
                    }
                }
            }
        },
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
