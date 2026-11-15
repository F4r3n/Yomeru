//! Settings card for the grammar dictionary: download it from its
//! publisher or import a ZIP, check for a newer revision, remove it.
//!
//! The dictionary is converted and stored on this device only (see
//! `yomeru_core::grammar`), so this card never talks to the Yomeru server.

use dioxus::prelude::*;

use crate::grammar::{self, GrammarMeta, Installed};

#[derive(Clone, PartialEq)]
enum State {
    Loading,
    Missing,
    Installed(GrammarMeta),
    /// The stored file can't be read (e.g. an older format).
    Unreadable(String),
}

#[component]
pub fn GrammarCard() -> Element {
    let mut state = use_signal(|| State::Loading);
    // What a running action is doing, e.g. "Downloading…".
    let mut busy = use_signal(|| None::<&'static str>);
    let mut message = use_signal(|| None::<(String, bool)>);
    let mut update = use_signal(|| None::<String>);

    use_future(move || async move {
        state.set(match grammar::load().await {
            Ok(Some(meta)) => State::Installed(meta),
            Ok(None) => State::Missing,
            Err(e) => State::Unreadable(e),
        });
    });

    let mut finish = move |result: Result<Installed, String>| {
        busy.set(None);
        match result {
            Ok(i) => {
                message.set(Some((
                    format!(
                        "Installed {} entries ({:.0} MB on this device).",
                        i.entries,
                        i.bytes as f64 / 1e6
                    ),
                    false,
                )));
                update.set(None);
                state.set(State::Installed(i.meta));
            }
            Err(e) => message.set(Some((e, true))),
        }
    };

    let on_download = move |_| {
        message.set(None);
        busy.set(Some("Downloading… (about 32 MB)"));
        spawn(async move {
            let result = match grammar::download_zip().await {
                Ok((zip, sha256)) => {
                    busy.set(Some("Converting… this takes a few seconds."));
                    grammar::install_zip(zip, Some(sha256)).await
                }
                Err(e) => Err(format!("Download failed: {e}")),
            };
            finish(result);
        });
    };

    let on_import = move |evt: Event<FormData>| {
        let Some(file) = evt.files().into_iter().next() else {
            return;
        };
        message.set(None);
        busy.set(Some("Reading the file…"));
        spawn(async move {
            let result = match file.read_bytes().await {
                Ok(bytes) => {
                    busy.set(Some("Converting… this takes a few seconds."));
                    grammar::install_zip(bytes.to_vec(), None).await
                }
                Err(e) => Err(format!("Could not read the file: {e}")),
            };
            finish(result);
        });
    };

    let on_check = move |_| {
        message.set(None);
        busy.set(Some("Checking…"));
        spawn(async move {
            match grammar::check_update().await {
                Ok(Some(rev)) => update.set(Some(rev)),
                Ok(None) => message.set(Some(("Up to date.".into(), false))),
                Err(e) => message.set(Some((format!("Couldn't check: {e}"), true))),
            }
            busy.set(None);
        });
    };

    let on_remove = move |_| {
        message.set(None);
        spawn(async move {
            match grammar::remove().await {
                Ok(()) => {
                    update.set(None);
                    state.set(State::Missing);
                }
                Err(e) => message.set(Some((e, true))),
            }
        });
    };

    let working = busy.read().is_some();
    let current = state.read().clone();

    rsx! {
        div { class: "card",
            div { class: "section-title", "Grammar" }
            h3 { style: "margin-bottom: 4px;", "Grammar dictionary" }
            p { class: "muted", style: "font-size: 13px; margin-bottom: 14px;",
                "Explains grammar patterns such as ようにする or わけではない. "
                "It is "
                a { href: grammar::DEFAULT_HOME_URL, target: "_blank", "Bee's Ultimate Grammar Dictionary" }
                ", which collects 14 grammar sources. Yomeru downloads it from its publisher and converts it "
                "on this device, where it stays: it is never sent to the Yomeru server or synced."
            }
            match current {
                State::Loading => rsx! { span { class: "hint", "Loading…" } },
                State::Missing | State::Unreadable(_) => rsx! {
                    if let State::Unreadable(e) = &current {
                        div { style: "margin-bottom: 10px;",
                            span { class: "error", "The installed dictionary can't be read: {e}" }
                        }
                    }
                    div { class: "row", style: "gap: 12px; flex-wrap: wrap;",
                        button { class: "primary", disabled: working, onclick: on_download,
                            "↓ Download (32 MB)"
                        }
                        ImportButton { disabled: working, onchange: on_import }
                    }
                },
                State::Installed(meta) => rsx! {
                    div { class: "form-row",
                        label { "{meta.title}" }
                        span { class: "hint",
                            if meta.revision.is_empty() { "Installed." } else { "Revision {meta.revision}." }
                        }
                        if !meta.attribution.is_empty() {
                            details {
                                summary { class: "hint", "Sources" }
                                p { class: "hint", "{meta.attribution}" }
                            }
                        }
                    }
                    div { class: "row", style: "gap: 12px; flex-wrap: wrap;",
                        if let Some(rev) = update.read().clone() {
                            button { class: "primary", disabled: working, onclick: on_download,
                                "↓ Update to {rev}"
                            }
                        } else if !meta.index_url.is_empty() {
                            button { class: "outline", disabled: working, onclick: on_check,
                                "Check for update"
                            }
                        }
                        ImportButton { disabled: working, onchange: on_import }
                        button { class: "link", disabled: working, onclick: on_remove, "Remove" }
                    }
                },
            }
            if let Some(what) = *busy.read() {
                div { style: "margin-top: 10px;", span { class: "hint", "{what}" } }
            }
            if let Some((msg, err)) = message.read().clone() {
                div { style: "margin-top: 10px;",
                    span { class: if err { "error" } else { "ok" }, "{msg}" }
                }
            }
        }
    }
}

/// A file picker for a Yomitan grammar ZIP.
#[component]
fn ImportButton(disabled: bool, onchange: EventHandler<Event<FormData>>) -> Element {
    rsx! {
        label { class: "hint", style: "display: flex; flex-direction: column; gap: 4px;",
            "Or import a Yomitan grammar ZIP"
            input {
                r#type: "file", accept: "application/zip,.zip",
                disabled,
                onchange: move |e| onchange.call(e),
                style: "max-width: 320px;",
            }
        }
    }
}
