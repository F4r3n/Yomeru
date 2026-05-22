use dioxus::prelude::*;
use serde_json::json;
use wasm_bindgen::JsCast;

use crate::api;
use crate::idb::{get_all_cards, put_cards};
use crate::settings::{load, save};
use crate::sync::{schedule_sync, sync_now};
use crate::types::SrsCard;

#[component]
pub fn SettingsTab() -> Element {
    let mut settings = use_signal(load);
    let mut saved = use_signal(|| false);
    let mut otp_sent = use_signal(|| false);
    let mut otp_code = use_signal(String::new);
    let mut sync_status = use_signal(|| Option::<(String, bool)>::None);
    let mut backup_status = use_signal(|| Option::<(String, bool)>::None);
    let mut sync_busy = use_signal(|| false);

    let save_settings = move |_| {
        let s = settings.read().clone();
        let _ = save(&s);
        saved.set(true);
    };

    let request_otp = move |_| {
        sync_busy.set(true);
        let s = settings.read().clone();
        spawn(async move {
            match api::request_otp(s.server_url.trim(), s.server_email.trim()).await {
                Ok(Some(token)) => {
                    // Dev-mode server skipped OTP and handed back a token.
                    let mut next = s.clone();
                    next.server_token = token;
                    let _ = save(&next);
                    settings.set(next);
                    otp_sent.set(false);
                    sync_status.set(Some((
                        "Authenticated (dev mode — no code required).".into(),
                        false,
                    )));
                }
                Ok(None) => otp_sent.set(true),
                Err(e) => sync_status.set(Some((e, true))),
            }
            sync_busy.set(false);
        });
    };

    let verify_otp = move |_| {
        sync_busy.set(true);
        let s = settings.read().clone();
        let code = otp_code.read().trim().to_string();
        spawn(async move {
            match api::verify_otp(s.server_url.trim(), s.server_email.trim(), &code).await {
                Ok(token) => {
                    let mut next = s.clone();
                    next.server_token = token;
                    let _ = save(&next);
                    settings.set(next);
                    otp_sent.set(false);
                    otp_code.set(String::new());
                    sync_status.set(Some(("Authenticated. You can now sync.".into(), false)));
                }
                Err(e) => sync_status.set(Some((e, true))),
            }
            sync_busy.set(false);
        });
    };

    let on_sync_now = move |_| {
        sync_busy.set(true);
        spawn(async move {
            match sync_now().await {
                Ok(msg) => sync_status.set(Some((msg, false))),
                Err(e) => sync_status.set(Some((e, true))),
            }
            sync_busy.set(false);
        });
    };

    let export_json = move |_| {
        spawn(async move {
            let cards = get_all_cards().await.unwrap_or_default();
            let n = cards.len();
            let payload = json!({
                "version": env!("CARGO_PKG_VERSION"),
                "exportedAt": js_sys::Date::now(),
                "cards": cards,
            });
            let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
            match trigger_download(&text, "yomeru-cards.json") {
                Ok(()) => backup_status.set(Some((
                    format!("Exported {n} card{}.", if n == 1 { "" } else { "s" }),
                    false,
                ))),
                Err(e) => backup_status.set(Some((format!("Export failed: {e}"), true))),
            }
        });
    };

    let on_import = move |evt: Event<FormData>| {
        let files = evt.files();
        let Some(file) = files.into_iter().next() else { return };
        spawn(async move {
            match file.read_string().await {
                Ok(text) => match import_cards_json(&text).await {
                    Ok((added, skipped)) => backup_status.set(Some((
                        format!("Imported {added} card(s), skipped {skipped} existing."),
                        false,
                    ))),
                    Err(e) => backup_status.set(Some((format!("Import failed: {e}"), true))),
                },
                Err(e) => backup_status.set(Some((format!("Could not read file: {e}"), true))),
            }
        });
    };

    let cur = settings.read().clone();
    let has_token = !cur.server_token.is_empty();
    let saved_flag = *saved.read();

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "Settings" }
                    div { class: "subtitle", "Tune the scheduler, back up cards, and configure sync." }
                }
            }

            // Scheduler
            div { class: "card",
                div { class: "section-title", "Scheduler" }
                h3 { style: "margin-bottom: 4px;", "Review tuning" }
                p { class: "muted", style: "font-size: 13px; margin-bottom: 14px;",
                    "FSRS knobs that affect how often cards reappear."
                }

                div { class: "form-row",
                    label { "Graduate after N successes" }
                    input {
                        r#type: "number", min: "0",
                        value: "{cur.graduation_reps}",
                        oninput: move |e| settings.write().graduation_reps = e.value().parse().unwrap_or(0),
                    }
                    span { class: "hint", "0 = never graduate" }
                }
                div { class: "form-row",
                    label { "Interval scale ×{cur.interval_scale:.2}" }
                    input {
                        r#type: "range", min: "0.25", max: "3", step: "0.25",
                        value: "{cur.interval_scale}",
                        oninput: move |e| settings.write().interval_scale = e.value().parse().unwrap_or(1.0),
                    }
                    span { class: "hint", "Lower = more frequent reviews." }
                }
                div { class: "form-row",
                    label { "Max cards per session" }
                    input {
                        r#type: "number", min: "1", max: "200",
                        value: "{cur.max_session_cards}",
                        oninput: move |e| settings.write().max_session_cards = e.value().parse().unwrap_or(20),
                    }
                }
                div { class: "row", style: "margin-top: 6px;",
                    button { class: "primary", onclick: save_settings, "Save changes" }
                    if saved_flag {
                        span { class: "ok", "✓ Saved" }
                    }
                }
            }

            // Backup
            div { class: "card",
                div { class: "section-title", "Backup" }
                h3 { style: "margin-bottom: 4px;", "Backup & restore" }
                p { class: "muted", style: "font-size: 13px; margin-bottom: 14px;",
                    "Export cards as JSON. Import merges; existing cards are kept."
                }
                div { class: "row", style: "gap: 12px; flex-wrap: wrap;",
                    button { onclick: export_json, "↓ Export JSON" }
                    input {
                        r#type: "file", accept: "application/json,.json",
                        onchange: on_import,
                        style: "max-width: 320px;",
                    }
                }
                if let Some((msg, err)) = backup_status.read().clone() {
                    div { style: "margin-top: 10px;",
                        span { class: if err { "error" } else { "ok" }, "{msg}" }
                    }
                }
            }

            // Sync
            div { class: "card",
                div { class: "section-title", "Sync" }
                h3 { style: "margin-bottom: 4px;",
                    "Sync server "
                    if has_token {
                        span { class: "badge active", style: "margin-left: 8px;", "Connected" }
                    } else {
                        span { class: "badge", style: "margin-left: 8px;", "Not connected" }
                    }
                }
                p { class: "muted", style: "font-size: 13px; margin-bottom: 14px;",
                    "Enter the server URL and email. A one-time code is emailed for auth."
                }

                div { class: "form-row",
                    label { "Server URL" }
                    input {
                        r#type: "url", placeholder: "http://localhost:8080",
                        value: "{cur.server_url}",
                        oninput: move |e| settings.write().server_url = e.value(),
                    }
                }
                div { class: "form-row",
                    label { "Email" }
                    input {
                        r#type: "email", placeholder: "your@email.com",
                        value: "{cur.server_email}",
                        oninput: move |e| settings.write().server_email = e.value(),
                    }
                }

                if !*otp_sent.read() {
                    div { class: "row", style: "gap: 8px;",
                        button {
                            onclick: request_otp,
                            disabled: *sync_busy.read(),
                            if *sync_busy.read() { "Sending…" } else { "Send code" }
                        }
                        button {
                            class: "primary",
                            onclick: on_sync_now,
                            disabled: *sync_busy.read() || !has_token,
                            if *sync_busy.read() { "Syncing…" } else { "Sync now" }
                        }
                    }
                } else {
                    div { class: "form-row",
                        label { "Verification code" }
                        div { class: "row", style: "gap: 8px;",
                            input {
                                r#type: "text", inputmode: "numeric", maxlength: "6",
                                placeholder: "000000",
                                value: "{otp_code}",
                                oninput: move |e| otp_code.set(e.value()),
                                style: "max-width: 160px; letter-spacing: 0.3em; text-align: center;",
                            }
                            button {
                                class: "primary",
                                onclick: verify_otp,
                                disabled: *sync_busy.read(),
                                if *sync_busy.read() { "Verifying…" } else { "Verify" }
                            }
                        }
                        span { class: "hint", "Check your email for the 6-digit code." }
                    }
                }
                if let Some((msg, err)) = sync_status.read().clone() {
                    div { style: "margin-top: 10px;",
                        span { class: if err { "error" } else { "ok" }, "{msg}" }
                    }
                }
            }
        }
    }
}

/// Triggers a download by creating a Blob URL on a transient <a>. Same pattern
/// the extension uses for JSON export.
fn trigger_download(content: &str, filename: &str) -> Result<(), String> {
    use wasm_bindgen::JsValue;
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let blob_parts = js_sys::Array::new();
    blob_parts.push(&JsValue::from_str(content));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/json");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &opts)
        .map_err(|_| "blob create failed".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "url create failed".to_string())?;
    let a = document
        .create_element("a")
        .map_err(|_| "create_element failed".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "anchor cast failed".to_string())?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    web_sys::Url::revoke_object_url(&url).ok();
    Ok(())
}

async fn import_cards_json(text: &str) -> Result<(usize, usize), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let arr = v
        .get("cards")
        .and_then(|c| c.as_array())
        .ok_or("missing 'cards' array")?;
    let mut added = 0usize;
    let mut skipped = 0usize;
    let existing = get_all_cards().await.map_err(|e| e.to_string())?;
    let existing_ids: std::collections::HashSet<String> =
        existing.into_iter().map(|c| c.id).collect();
    let mut to_put = Vec::new();
    for c in arr {
        let card: Result<SrsCard, _> = serde_json::from_value(c.clone());
        let Ok(card) = card else {
            skipped += 1;
            continue;
        };
        if existing_ids.contains(&card.id) {
            skipped += 1;
            continue;
        }
        to_put.push(card);
        added += 1;
    }
    put_cards(&to_put).await.map_err(|e| e.to_string())?;
    if added > 0 {
        schedule_sync();
    }
    Ok((added, skipped))
}
