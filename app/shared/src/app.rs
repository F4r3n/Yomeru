use std::rc::Rc;

use dioxus::history::History;
use dioxus::prelude::*;
use dioxus::router::components::HistoryProvider;
use dioxus::web::WebHistory;
use gloo_storage::{LocalStorage, Storage};

use crate::routes::{about, lookup, new_words, review, settings, word_list};
use crate::sync::SyncGen;
use crate::theme::global_css;

const THEME_KEY: &str = "yomeru.theme";

#[derive(Clone, Routable, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Review {},
        #[route("/new")]
        New {},
        #[route("/words")]
        Words {},
        #[nest("/lookup")]
            #[layout(LookupLayout)]
                #[route("/")]
                Lookup {},
                #[route("/:word")]
                LookupDetail { word: String },
            #[end_layout]
        #[end_nest]
        #[route("/settings")]
        Settings {},
        #[route("/about")]
        About {},
}

#[component]
pub fn App() -> Element {
    // Reactive generation that route load-effects subscribe to. Bumped once
    // the startup sync below lands so the mounted tab reloads from IDB.
    use_context_provider(|| SyncGen(Signal::new(0)));

    // Pull remote changes once when the app opens. Previously a sync only ran
    // after a local mutation (debounced) or the manual "Sync now" button, so a
    // freshly opened device showed stale data until the user synced by hand.
    // `sync_now` is a no-op error ("not authenticated") when no server token
    // is configured, so this is safe to fire unconditionally. No reactive
    // reads in the future (bump uses `peek`), so this runs exactly once.
    use_future(move || async move {
        match crate::sync::sync_now().await {
            Ok(_) => crate::sync::bump_sync_generation(),
            Err(e) => log::debug!("[yomeru] startup sync skipped: {e}"),
        }
    });

    rsx! {
        document::Style { {global_css()} }
        // Disable Dioxus' default scroll-to-(0,0) on every navigation so
        // expanding a result card in /lookup doesn't yank the page to the
        // top. Manual scroll restoration on browser back is sacrificed —
        // acceptable trade-off for the SPA flow we have today.
        HistoryProvider {
            history: |_| Rc::new(WebHistory::new(None, false)) as Rc<dyn History>,
            Router::<Route> {}
        }
    }
}

fn load_theme() -> String {
    LocalStorage::get::<String>(THEME_KEY).unwrap_or_else(|_| "dark".to_string())
}

fn apply_theme_to_dom(theme: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(root) = doc.document_element() {
            let _ = root.set_attribute("data-theme", theme);
        }
    }
}

#[component]
fn Shell() -> Element {
    let mut theme = use_signal(load_theme);

    use_effect(move || {
        apply_theme_to_dom(&theme.read());
        let _ = LocalStorage::set(THEME_KEY, theme.read().clone());
    });

    let toggle_theme = move |_| {
        let next = if *theme.read() == "light" { "dark" } else { "light" };
        theme.set(next.to_string());
    };

    let is_light = *theme.read() == "light";
    let toggle_label = if is_light { "Switch to dark theme" } else { "Switch to light theme" };
    let toggle_icon = if is_light { "☀" } else { "☾" };

    rsx! {
        div { class: "app-shell",
            div { class: "topbar",
                Link { to: Route::Review {}, class: "brand",
                    span { class: "mark", "読" }
                    span { class: "name", "Yomeru" }
                    span { class: "tag", "Japanese reader & SRS" }
                }
                div { class: "topbar-actions",
                    button {
                        class: "icon-btn",
                        title: "{toggle_label}",
                        "aria-label": "{toggle_label}",
                        onclick: toggle_theme,
                        "{toggle_icon}"
                    }
                }
            }
            nav { class: "sidebar",
                NavTab { to: Route::Review {},   icon: "▶", label: "Review" }
                NavTab { to: Route::New {},      icon: "✦", label: "New Words" }
                NavTab { to: Route::Words {},    icon: "≡", label: "Word List" }
                NavTab { to: Route::Lookup {},   icon: "⌕", label: "Lookup" }
                NavTab { to: Route::Settings {}, icon: "⚙", label: "Settings" }
                NavTab { to: Route::About {},    icon: "ⓘ", label: "About" }
            }
            main { class: "content",
                div { class: "content-inner",
                    Outlet::<Route> {}
                }
            }
        }
    }
}

#[component]
fn NavTab(to: Route, icon: &'static str, label: &'static str) -> Element {
    let current = use_route::<Route>();
    let active = nav_active(&to, &current);
    let class = if active { "nav-tab active" } else { "nav-tab" };
    rsx! {
        Link { class: "{class}", to: to,
            span { class: "nav-icon", "{icon}" }
            span { "{label}" }
        }
    }
}

fn nav_active(to: &Route, current: &Route) -> bool {
    if std::mem::discriminant(to) == std::mem::discriminant(current) {
        return true;
    }
    // The Lookup tab should stay highlighted on the LookupDetail sub-route.
    matches!((to, current), (Route::Lookup {}, Route::LookupDetail { .. }))
}

#[component]
fn Review() -> Element {
    rsx! { review::ReviewTab {} }
}
#[component]
fn New() -> Element {
    rsx! { new_words::NewWordsTab {} }
}
#[component]
fn Words() -> Element {
    rsx! { word_list::WordListTab {} }
}
#[component]
fn LookupLayout() -> Element {
    rsx! { lookup::LookupLayout {} }
}
#[component]
fn Lookup() -> Element {
    rsx! { lookup::LookupEmpty {} }
}
#[component]
fn LookupDetail(word: String) -> Element {
    rsx! { lookup::LookupDetailPane { word } }
}
#[component]
fn Settings() -> Element {
    rsx! { settings::SettingsTab {} }
}
#[component]
fn About() -> Element {
    rsx! { about::AboutTab {} }
}
