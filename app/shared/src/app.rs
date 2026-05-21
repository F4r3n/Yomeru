use dioxus::prelude::*;

use crate::routes::{about, lookup, new_words, review, settings, word_list};
use crate::theme::GLOBAL_CSS;

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
        #[route("/lookup")]
        Lookup {},
        #[route("/settings")]
        Settings {},
        #[route("/about")]
        About {},
}

#[component]
pub fn App() -> Element {
    rsx! {
        document::Style { {GLOBAL_CSS} }
        Router::<Route> {}
    }
}

#[component]
fn Shell() -> Element {
    rsx! {
        div { class: "app-shell",
            header {
                div { class: "header-top",
                    h1 { "Yomeru" }
                }
                nav {
                    TabLink { to: Route::Review {},   label: "Review" }
                    TabLink { to: Route::New {},      label: "New Words" }
                    TabLink { to: Route::Words {},    label: "Word List" }
                    TabLink { to: Route::Lookup {},   label: "Lookup" }
                    TabLink { to: Route::Settings {}, label: "Settings" }
                    TabLink { to: Route::About {},    label: "About" }
                }
            }
            main { Outlet::<Route> {} }
        }
    }
}

#[component]
fn TabLink(to: Route, label: &'static str) -> Element {
    let current = use_route::<Route>();
    let active = std::mem::discriminant(&current) == std::mem::discriminant(&to);
    let class = if active { "tab active" } else { "tab" };
    rsx! {
        Link { class: "{class}", to: to, "{label}" }
    }
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
fn Lookup() -> Element {
    rsx! { lookup::LookupTab {} }
}
#[component]
fn Settings() -> Element {
    rsx! { settings::SettingsTab {} }
}
#[component]
fn About() -> Element {
    rsx! { about::AboutTab {} }
}
