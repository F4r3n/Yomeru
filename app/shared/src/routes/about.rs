use dioxus::prelude::*;

#[component]
pub fn AboutTab() -> Element {
    let version = env!("CARGO_PKG_VERSION");

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "About Yomeru" }
                    div { class: "subtitle", "A Japanese reader with spaced-repetition vocabulary memory." }
                }
                div { class: "actions",
                    span { class: "pill", "v{version}" }
                }
            }

            div { class: "card",
                div { class: "section-title", "Data sources" }
                h3 { style: "margin-bottom: 10px;", "Dictionaries & corpora" }
                div { class: "col", style: "gap: 10px;",
                    div { class: "row", style: "justify-content: space-between; align-items: baseline;",
                        div {
                            div { style: "font-weight: 600;", "JMdict" }
                            div { class: "muted", style: "font-size: 13px;", "Japanese-Multilingual Dictionary" }
                        }
                        a { href: "https://www.edrdg.org/jmdict/j_jmdict.html", target: "_blank",
                            class: "link", "EDRDG ↗" }
                    }
                    hr { class: "divider" }
                    div { class: "row", style: "justify-content: space-between; align-items: baseline;",
                        div {
                            div { style: "font-weight: 600;", "KANJIDIC" }
                            div { class: "muted", style: "font-size: 13px;", "Kanji information" }
                        }
                        a { href: "https://www.edrdg.org/wiki/index.php/KANJIDIC_Project", target: "_blank",
                            class: "link", "EDRDG ↗" }
                    }
                    hr { class: "divider" }
                    div { class: "row", style: "justify-content: space-between; align-items: baseline;",
                        div {
                            div { style: "font-weight: 600;", "Tatoeba" }
                            div { class: "muted", style: "font-size: 13px;", "Example sentences" }
                        }
                        a { href: "https://tatoeba.org/", target: "_blank",
                            class: "link", "tatoeba.org ↗" }
                    }
                }
                p { class: "muted", style: "margin-top: 14px; font-size: 12px;",
                    "EDRDG files are licensed under CC BY-SA 4.0."
                }
            }

            div { class: "card",
                div { class: "section-title", "Scheduler" }
                h3 { style: "margin-bottom: 4px;", "FSRS-4.5" }
                p { class: "muted", style: "font-size: 13px;",
                    "Reviews use the FSRS-4.5 algorithm via "
                    a { href: "https://github.com/open-spaced-repetition/rs-fsrs", target: "_blank",
                        class: "link", "rs-fsrs ↗" }
                    ". Tune the parameters from the Settings tab."
                }
            }
        }
    }
}
