use dioxus::prelude::*;
use jmdict_types::{PartOfSpeech, WordEntry};

use crate::dict::{frequency_label, preferred_headword, primary_reading};

pub fn pos_label(p: &PartOfSpeech) -> String {
    // Debug repr is "Adjective" / "AdjectiveNa" etc — readable enough for now.
    format!("{p:?}")
}

pub fn pos_list(ps: &[PartOfSpeech]) -> String {
    ps.iter().map(pos_label).collect::<Vec<_>>().join(", ")
}

#[component]
pub fn EntryCard(
    entry: WordEntry,
    on_add: Option<EventHandler<u32>>,
    on_select: Option<EventHandler<()>>,
    #[props(default)] is_added: bool,
) -> Element {
    let title = preferred_headword(&entry).to_string();
    let reading = primary_reading(&entry).to_string();
    // When the title is the kana reading (kana-preferred entry), surface the
    // kanji writing underneath in smaller text. Otherwise show the reading.
    let kana_titled = title == reading;
    let sub_kanji = if kana_titled {
        entry.kanji_forms.first().map(|k| k.text.clone())
    } else {
        None
    };
    let show_reading = !kana_titled && !reading.is_empty() && reading != title;
    let freq = frequency_label(&entry);
    let on_add_for = entry.sequence;
    let sense_count = entry.senses.len();
    let clickable = on_select.is_some();
    let card_class = if clickable { "card clickable" } else { "card" };

    rsx! {
        div {
            class: "{card_class}",
            onclick: move |_| {
                if let Some(s) = on_select {
                    s.call(());
                }
            },
            div { class: "row", style: "justify-content: space-between; align-items: baseline; margin-bottom: 10px;",
                div {
                    div { class: "row", style: "align-items: baseline; gap: 8px;",
                        div { class: "headword", "{title}" }
                        if let Some(k) = sub_kanji {
                            div { class: "kanji-sub", "{k}" }
                        }
                        if let Some(f) = freq {
                            span { class: "freq-badge", "{f}" }
                        }
                    }
                    if show_reading {
                        div { class: "reading", "{reading}" }
                    }
                }
                if let Some(handler) = on_add {
                    if is_added {
                        button {
                            class: "success",
                            disabled: true,
                            onclick: move |e| e.stop_propagation(),
                            "✓ Added"
                        }
                    } else {
                        button {
                            class: "primary",
                            onclick: move |e| {
                                e.stop_propagation();
                                handler.call(on_add_for);
                            },
                            "+ Add"
                        }
                    }
                }
            }
            for (si, sense) in entry.senses.iter().enumerate() {
                {
                    let needs_divider = si > 0;
                    rsx! {
                        if needs_divider {
                            hr { class: "divider" }
                        }
                        div {
                            if !sense.pos.is_empty() {
                                div { class: "pos", "{pos_list(&sense.pos)}" }
                            }
                            for gloss in sense.glosses.iter() {
                                div { class: "gloss",
                                    if sense_count > 1 {
                                        span { class: "muted", style: "margin-right: 4px;", "{si + 1}." }
                                    }
                                    "{gloss.text}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
