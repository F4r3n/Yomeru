//! The privacy policy (GDPR art. 13). The text is the same for every server;
//! who runs it and how to reach them come from the configured sync server
//! (`/api/privacy`), since each self-hosted server has its own operator.

use dioxus::prelude::*;

use crate::api::{PrivacyInfo, privacy_info};
use crate::app::Route;
use crate::settings::load;

const UPDATED: &str = "8 November 2026";

#[component]
pub fn PrivacyPage() -> Element {
    let server_url = load().server_url.trim().to_string();
    let url_for_fetch = server_url.clone();
    let info = use_resource(move || {
        let url = url_for_fetch.clone();
        async move {
            if url.is_empty() {
                return None;
            }
            privacy_info(&url).await.ok()
        }
    });
    let info: Option<PrivacyInfo> = info.read().clone().flatten();
    let controller = info.as_ref().and_then(|i| i.controller.clone());
    let contact = info.as_ref().and_then(|i| i.contact.clone());

    rsx! {
        div { class: "privacy",
            div { class: "card",
                div { class: "section-title", "Privacy policy" }
                h3 { style: "margin-bottom: 4px;", "Your data in Yomeru" }
                p { class: "muted", style: "font-size: 13px;", "Last updated {UPDATED}." }

                h4 { "Who is responsible" }
                if server_url.is_empty() {
                    p {
                        "Without a sync server, nobody but you holds your data: everything stays on this device. "
                        "If you set up sync, the operator of that server is responsible for what it stores; "
                        "this page then shows their details."
                    }
                } else {
                    p {
                        "Your data on the sync server "
                        code { "{server_url}" }
                        " is handled by its operator:"
                    }
                    ul {
                        li {
                            strong { "Data controller: " }
                            match &controller {
                                Some(c) => rsx! { "{c}" },
                                None => rsx! { span { class: "muted", "not provided by this server" } },
                            }
                        }
                        li {
                            strong { "Contact for data requests: " }
                            match &contact {
                                Some(c) if c.contains('@') && !c.contains(' ') => rsx! {
                                    a { class: "link", href: "mailto:{c}", "{c}" }
                                },
                                Some(c) => rsx! { "{c}" },
                                None => rsx! { span { class: "muted", "not provided by this server" } },
                            }
                        }
                    }
                }

                h4 { "Without an account" }
                p {
                    "Your cards, review history and settings are stored only on your device. "
                    "There are no ads, no analytics and no tracking, and no cookies: the app keeps "
                    "its data in the browser's or device's local storage only so that it works."
                }
                p {
                    "On the website, words you look up are sent to the server so it can search the "
                    "dictionary. They are answered and not stored."
                }

                h4 { "What the server stores when you sync" }
                ul {
                    li { strong { "Your email address" } ", to identify your account and send you sign-in codes. Nothing else is ever sent to it." }
                    li { strong { "Your cards" } ": which words and kanji you study, when you added them, and their review schedule." }
                    li { strong { "Your review history" } ": for each answer, the card, the button you pressed and when." }
                    li { strong { "Your review settings" } ", and short-lived markers for cards you deleted, so the deletion reaches your other devices." }
                    li { strong { "Sign-in data" } ": sessions, stored only as a hash, and a one-time code while you sign in." }
                }
                p {
                    "In the Firefox extension, nothing is sent until you allow it when you first sign in; "
                    "you can withdraw that permission at any time in about:addons, which pauses sync."
                }
                p {
                    "Your IP address is used for a moment to limit abusive traffic and is not stored by Yomeru. "
                    "The web server in front of it may keep standard access logs, under its operator's rules."
                }

                h4 { "Why, and on what basis" }
                p {
                    "Only to provide the sync you asked for: keeping your deck the same on all your devices. "
                    "The legal basis is the performance of that service (GDPR art. 6(1)(b)). "
                    "Your data is never sold, shared or used for anything else. The server's hosting "
                    "and email providers handle it only on the operator's behalf."
                }

                h4 { "How long it is kept" }
                ul {
                    li { "Your account data: until you delete your account." }
                    li { "Sign-in codes: 10 minutes. Sessions: 30 days." }
                    li { "Deletion markers: 90 days." }
                }

                h4 { "Your rights" }
                p { "You can at any time:" }
                ul {
                    li {
                        strong { "Delete your account" }
                        ": "
                        Link { to: Route::Settings {}, class: "link", "Settings" }
                        " → Sync → Delete account, then enter the code we email you. Everything listed above is erased from the server immediately."
                    }
                    li {
                        strong { "Get a copy of your data" }
                        ": Settings → Backup → Export JSON gives you your cards. "
                        "For a full copy of what the server holds, including review history, write to the contact above."
                    }
                    li { strong { "Correct, restrict or object" } " to how your data is used, by writing to the contact above." }
                    li {
                        strong { "Complain" }
                        " to your data protection authority, in France the "
                        a { class: "link", href: "https://www.cnil.fr/", target: "_blank", "CNIL ↗" }
                        "."
                    }
                }
            }
        }
    }
}
