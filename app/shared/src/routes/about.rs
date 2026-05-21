use dioxus::prelude::*;

#[component]
pub fn AboutTab() -> Element {
    rsx! {
        div { class: "col",
            h2 { "About Yomeru" }
            p { class: "muted",
                "Yomeru is a Japanese reader with spaced-repetition vocabulary memory."
            }
            div { class: "card",
                h3 { "Data sources" }
                ul { style: "margin-left: 18px; color: var(--subtext);",
                    li {
                        "JMdict — Japanese-Multilingual Dictionary, "
                        a { href: "https://www.edrdg.org/jmdict/j_jmdict.html", target: "_blank",
                            style: "color: var(--blue);", "EDRDG" }
                    }
                    li {
                        "KANJIDIC — kanji information, "
                        a { href: "https://www.edrdg.org/wiki/index.php/KANJIDIC_Project", target: "_blank",
                            style: "color: var(--blue);", "EDRDG" }
                    }
                    li {
                        "Tatoeba — example sentences, "
                        a { href: "https://tatoeba.org/", target: "_blank",
                            style: "color: var(--blue);", "tatoeba.org" }
                    }
                }
                p { class: "muted", style: "margin-top: 8px;",
                    "Both EDRDG files are licensed under CC BY-SA 4.0."
                }
            }
            div { class: "card",
                h3 { "Scheduler" }
                p { class: "muted",
                    "Reviews use the FSRS-4.5 algorithm via "
                    a { href: "https://github.com/open-spaced-repetition/rs-fsrs", target: "_blank",
                        style: "color: var(--blue);", "rs-fsrs" }
                    "."
                }
            }
        }
    }
}
