//! Pieces shared by the word and kanji review tabs: the answer/rating buttons
//! and the keyboard shortcuts that drive them.

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;

pub(crate) fn rating_label(r: u8) -> &'static str {
    match r {
        1 => "Again",
        2 => "Hard",
        4 => "Easy",
        _ => "Good",
    }
}

fn event_target(e: &web_sys::KeyboardEvent) -> Option<web_sys::HtmlElement> {
    e.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
}

/// Whether a key event is aimed at a text field, where digits and spaces are
/// input rather than review shortcuts. Checkboxes, radios and other non-text
/// inputs don't count: they would otherwise switch the shortcuts off silently
/// for as long as they keep focus.
fn typing_in_field(e: &web_sys::KeyboardEvent) -> bool {
    event_target(e).is_some_and(|el| match el.tag_name().as_str() {
        "TEXTAREA" | "SELECT" => true,
        "INPUT" => !matches!(
            el.get_attribute("type")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "checkbox" | "radio" | "button" | "submit" | "reset" | "range" | "color" | "file"
        ),
        _ => el.is_content_editable(),
    })
}

/// The rating (1–4) a key press stands for, by physical key so the digit row
/// works on any layout (AZERTY types `&é"'` there unshifted). Numpad keys only
/// count with NumLock on — off, they are End/↓/PgDn/← and mean scrolling.
fn rating_key(e: &web_sys::KeyboardEvent) -> Option<u8> {
    let code = e.code();
    let digit = if let Some(d) = code.strip_prefix("Digit") {
        d
    } else {
        code.strip_prefix("Numpad")
            .filter(|_| e.key() == code["Numpad".len()..])?
    };
    match digit.parse() {
        Ok(r @ 1..=4) => Some(r),
        _ => None,
    }
}

/// Desktop shortcuts for a review session: Space/Enter reveals the answer,
/// 1–4 rate it. `in_session` says whether a card is on screen; `show_back`
/// whether its answer is showing.
///
/// Bound on the document so the keys work without focusing anything; the
/// listeners are removed when the calling component is dropped. Call it
/// unconditionally, like any hook.
pub(crate) fn use_review_shortcuts(
    in_session: impl Fn() -> bool + 'static,
    show_back: Signal<bool>,
    mut reveal: impl FnMut() + 'static,
    mut rate: impl FnMut(u8) + 'static,
) {
    use_hook(move || {
        let document = web_sys::window().and_then(|w| w.document())?;
        // The DOM listener runs outside Dioxus, where `spawn` (used by both
        // actions) has no runtime. Re-entering the component's scope runs them
        // synchronously, against the same state the checks below just read.
        // Held weakly: the runtime owns this hook, so a strong handle would
        // keep both alive in a cycle.
        let runtime = Rc::downgrade(&dioxus::core::Runtime::current());
        let scope = dioxus::core::current_scope_id();
        // Whether focus last moved by Tab rather than a click. A control the
        // user tabbed to keeps its native Space/Enter; one merely left focused
        // by a click must not swallow the reveal. Tracked by hand because
        // `:focus-visible` turns on for mouse-focused elements as soon as any
        // key is pressed in Chromium.
        let tabbed = Rc::new(Cell::new(false));
        let on_pointer = {
            let tabbed = tabbed.clone();
            gloo_events::EventListener::new(&document, "pointerdown", move |_| tabbed.set(false))
        };
        let on_key = gloo_events::EventListener::new(&document, "keydown", move |e| {
            let Some(e) = e.dyn_ref::<web_sys::KeyboardEvent>() else {
                return;
            };
            if e.key() == "Tab" {
                tabbed.set(true);
                return;
            }
            // Leave browser shortcuts alone, don't let a held key rate a run of
            // cards, and never steal keys being typed into a field. Shift is
            // allowed: AZERTY users reach the digits with it.
            if e.ctrl_key() || e.alt_key() || e.meta_key() || e.repeat() || typing_in_field(e) {
                return;
            }
            let Some(runtime) = runtime.upgrade().filter(|_| in_session()) else {
                return;
            };
            if *show_back.peek() {
                let Some(r) = rating_key(e) else {
                    return;
                };
                e.prevent_default();
                runtime.in_scope(scope, || rate(r));
            } else {
                let on_control =
                    tabbed.get() && event_target(e).is_some_and(|el| el.tag_name() != "BODY");
                if !matches!(e.key().as_str(), " " | "Enter") || on_control {
                    return;
                }
                // Otherwise Space also scrolls the page, and re-clicks a button
                // that kept focus from a mouse click.
                e.prevent_default();
                runtime.in_scope(scope, &mut reveal);
            }
        });
        Some(Rc::new((on_key, on_pointer)))
    });
}

/// The full-width "Show answer" button, with its shortcut hint.
#[component]
pub(crate) fn ShowAnswerButton(on_reveal: EventHandler<()>) -> Element {
    rsx! {
        button { class: "primary", style: "width: 100%;",
            onclick: move |_| on_reveal.call(()),
            "Show answer"
            kbd { "aria-hidden": "true", "Space" }
        }
    }
}

/// Again/Hard/Good/Easy, with their shortcut hints. `disabled` while a rating
/// is being saved.
#[component]
pub(crate) fn RateButtons(disabled: bool, on_rate: EventHandler<u8>) -> Element {
    rsx! {
        div { class: "rate-grid",
            for (r, class) in [(1u8, "danger"), (2, "warning"), (3, "success"), (4, "primary")] {
                button { key: "{r}", class, disabled,
                    onclick: move |_| on_rate.call(r),
                    "{rating_label(r)}"
                    kbd { "aria-hidden": "true", "{r}" }
                }
            }
        }
    }
}

#[component]
pub(crate) fn TabButton(
    active: bool,
    onclick: EventHandler<MouseEvent>,
    #[props(into)] label: String,
) -> Element {
    let class = if active { "active" } else { "" };
    rsx! {
        button { class: "{class}", onclick: move |e| onclick.call(e), "{label}" }
    }
}
