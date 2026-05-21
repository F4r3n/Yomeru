use dioxus::prelude::*;
use jmdict_types::{PartOfSpeech, WordEntry};

use crate::dict::{primary_headword, primary_reading};

pub fn pos_label(p: &PartOfSpeech) -> String {
    // Debug repr is "Adjective" / "AdjectiveNa" etc — readable enough for now.
    format!("{p:?}")
}

pub fn pos_list(ps: &[PartOfSpeech]) -> String {
    ps.iter().map(pos_label).collect::<Vec<_>>().join(", ")
}

#[component]
pub fn EntryCard(entry: WordEntry, on_add: Option<EventHandler<String>>) -> Element {
    let headword = primary_headword(&entry).to_string();
    let reading = primary_reading(&entry).to_string();
    let show_reading = !reading.is_empty() && reading != headword;
    let on_add_for = headword.clone();

    rsx! {
        div { class: "card",
            div { class: "row", style: "justify-content: space-between; align-items: baseline;",
                div {
                    div { class: "headword", "{headword}" }
                    if show_reading {
                        div { class: "reading", "{reading}" }
                    }
                }
                if let Some(handler) = on_add {
                    button {
                        class: "primary",
                        onclick: move |_| handler.call(on_add_for.clone()),
                        "+ add"
                    }
                }
            }
            for sense in entry.senses.iter() {
                if !sense.pos.is_empty() {
                    div { class: "pos", "{pos_list(&sense.pos)}" }
                }
                for gloss in sense.glosses.iter() {
                    div { class: "gloss", "• {gloss.text}" }
                }
            }
        }
    }
}
