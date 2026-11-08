use dioxus::prelude::*;
use serde_json::json;

use crate::api;
use crate::app::Route;
use crate::cards_io::{format_import_result, import_cards_json, trigger_download};
use crate::idb::get_all_cards;
use crate::settings::{load, save};
use crate::sync::{SyncHealth, bump_sync_generation, clear_sync_health, sync_now};
use crate::types::CARDS_SCHEMA_VERSION;

/// localStorage key holding the email a pending OTP was sent to. Survives a
/// popup close (extension popups are destroyed on blur), so reopening returns
/// to the code-entry step instead of forcing a re-send — which would otherwise
/// trip the server's resend rate limit.
const OTP_PENDING_KEY: &str = "yomeru.otp_pending";

#[component]
pub fn SettingsTab() -> Element {
    let mut settings = use_signal(load);
    let mut saved = use_signal(|| false);
    // Restore the "awaiting code" step if a code was sent before the popup
    // closed — but only for the email still configured, so a stale flag from a
    // different account can't strand the user on a dead code field.
    let mut otp_sent = use_signal(|| {
        let pending: String = crate::rt::kv_get(OTP_PENDING_KEY).unwrap_or_default();
        !pending.is_empty() && pending == load().server_email.trim()
    });
    let mut otp_code = use_signal(String::new);
    let mut sync_status = use_signal(|| Option::<(String, bool)>::None);
    let mut backup_status = use_signal(|| Option::<(String, bool)>::None);
    let mut sync_busy = use_signal(|| false);
    // Account deletion: a code has been emailed, and the code typed so far.
    let mut delete_pending = use_signal(|| false);
    let mut delete_code = use_signal(String::new);
    let mut delete_busy = use_signal(|| false);
    let mut delete_status = use_signal(|| Option::<(String, bool)>::None);
    // Extension only: Firefox's data consent for sync. Elsewhere always true.
    let mut consent = use_resource(|| async { api::sync_consent().await });
    let health = use_context::<SyncHealth>();

    let save_settings = move |_| {
        let s = settings.read().clone();
        let _ = save(&s);
        saved.set(true);
    };

    let request_otp = move |_| {
        let s = settings.read().clone();
        if s.server_url.trim().is_empty() || s.server_email.trim().is_empty() {
            sync_status.set(Some(("Enter the server URL and email first.".into(), true)));
            return;
        }
        sync_busy.set(true);
        spawn(async move {
            match api::request_otp(s.server_url.trim(), s.server_email.trim()).await {
                Ok(Some(token)) => {
                    // Dev-mode server skipped OTP and handed back a token.
                    let mut next = s.clone();
                    next.server_token = token;
                    let _ = save(&next);
                    forget_sync_cursor().await;
                    settings.set(next);
                    crate::rt::kv_delete(OTP_PENDING_KEY);
                    otp_sent.set(false);
                    sync_status.set(Some((
                        "Authenticated (dev mode — no code required).".into(),
                        false,
                    )));
                }
                Ok(None) => {
                    let _ = crate::rt::kv_set(OTP_PENDING_KEY, &s.server_email.trim());
                    otp_sent.set(true);
                }
                Err(e) => sync_status.set(Some((e, true))),
            }
            sync_busy.set(false);
        });
    };

    let verify_otp = move |_| {
        let s = settings.read().clone();
        let code = otp_code.read().trim().to_string();
        if code.is_empty() {
            sync_status.set(Some(("Enter the code from your email.".into(), true)));
            return;
        }
        sync_busy.set(true);
        spawn(async move {
            match api::verify_otp(s.server_url.trim(), s.server_email.trim(), &code).await {
                Ok(token) => {
                    let mut next = s.clone();
                    next.server_token = token;
                    let _ = save(&next);
                    forget_sync_cursor().await;
                    settings.set(next);
                    crate::rt::kv_delete(OTP_PENDING_KEY);
                    otp_sent.set(false);
                    otp_code.set(String::new());
                    clear_sync_health();
                    // Sync straight away: whatever piled up while the old
                    // session was dead goes out now, not on the next edit.
                    let msg = match sync_now().await {
                        Ok(m) => {
                            bump_sync_generation();
                            format!("Authenticated. {m}")
                        }
                        Err(e) => format!("Authenticated, but sync failed: {e}"),
                    };
                    sync_status.set(Some((msg, false)));
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
                Ok(msg) => {
                    bump_sync_generation();
                    sync_status.set(Some((msg, false)));
                }
                Err(e) => sync_status.set(Some((e, true))),
            }
            sync_busy.set(false);
        });
    };

    // Account deletion, step 1: confirm, then have a code emailed. The
    // session alone can't delete — it might be a stolen token or someone
    // else at an unlocked device — so step 2 needs that code.
    let on_delete_account = move |_| {
        let s = settings.read().clone();
        spawn(async move {
            let confirmed = crate::rt::confirm(
                "Delete your account on this server?\n\nYour cards, review history and settings will be erased from the server, for every device. The cards on this device are kept.\n\nA confirmation code will be emailed to you first.",
            )
            .await;
            if !confirmed {
                return;
            }
            delete_busy.set(true);
            match api::request_account_deletion(s.server_url.trim(), &s.server_token).await {
                Ok(dev_code) => {
                    delete_code.set(dev_code.clone().unwrap_or_default());
                    delete_pending.set(true);
                    let msg = if dev_code.is_some() {
                        "Dev server: code filled in, no email sent.".to_string()
                    } else {
                        format!(
                            "Code sent to {}. Enter it below to delete.",
                            s.server_email.trim()
                        )
                    };
                    delete_status.set(Some((msg, false)));
                }
                Err(e) => delete_status.set(Some((format!("Couldn't start deletion: {e}"), true))),
            }
            delete_busy.set(false);
        });
    };

    // Step 2: the code erases the account.
    let on_confirm_delete = move |_| {
        let s = settings.read().clone();
        let code = delete_code.read().trim().to_string();
        spawn(async move {
            delete_busy.set(true);
            match api::delete_account(s.server_url.trim(), &s.server_token, &code).await {
                Ok(()) => {
                    // Signed out: without a token nothing syncs, here or in
                    // the extension's background. The email stays filled in.
                    let mut next = s.clone();
                    next.server_token.clear();
                    let _ = save(&next);
                    forget_sync_cursor().await;
                    clear_sync_health();
                    settings.set(next);
                    delete_pending.set(false);
                    delete_code.set(String::new());
                    delete_status.set(Some((
                        "Account deleted. This device keeps its cards and no longer syncs.".into(),
                        false,
                    )));
                }
                Err(e) => delete_status.set(Some((format!("Delete failed: {e}"), true))),
            }
            delete_busy.set(false);
        });
    };

    // Re-grants sync's data consent (extension). The prompt call must be the
    // first thing the task does, while Firefox still sees the click.
    let on_allow_sync = move |_| {
        spawn(async move {
            if !api::request_sync_consent().await {
                return;
            }
            consent.restart();
            match sync_now().await {
                Ok(msg) => {
                    bump_sync_generation();
                    sync_status.set(Some((msg, false)));
                }
                Err(e) => sync_status.set(Some((e, true))),
            }
        });
    };

    let export_json = move |_| {
        spawn(async move {
            let cards = get_all_cards().await.unwrap_or_default();
            let n = cards.len();
            let payload = json!({
                "schema": CARDS_SCHEMA_VERSION,
                "version": env!("CARGO_PKG_VERSION"),
                "exportedAt": crate::rt::now_ms(),
                "cards": cards,
            });
            let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
            match trigger_download(&text, "yomeru-cards.json") {
                Ok(saved_to) => backup_status.set(Some((
                    format!(
                        "Exported {n} card{}.{}",
                        if n == 1 { "" } else { "s" },
                        saved_to
                            .map(|p| format!(" Saved to {p}."))
                            .unwrap_or_default()
                    ),
                    false,
                ))),
                Err(e) => backup_status.set(Some((format!("Export failed: {e}"), true))),
            }
        });
    };

    let on_import = move |evt: Event<FormData>| {
        let files = evt.files();
        let Some(file) = files.into_iter().next() else {
            return;
        };
        spawn(async move {
            match file.read_string().await {
                Ok(text) => match import_cards_json(&text).await {
                    Ok((added, skips)) => {
                        backup_status.set(Some((format_import_result(added, skips), false)))
                    }
                    Err(e) => backup_status.set(Some((format!("Import failed: {e}"), true))),
                },
                Err(e) => backup_status.set(Some((format!("Could not read file: {e}"), true))),
            }
        });
    };

    let cur = settings.read().clone();
    let has_token = !cur.server_token.is_empty();
    let consent_missing = has_token && matches!(*consent.read(), Some(false));
    let saved_flag = *saved.read();
    let retention_pct = (cur.request_retention * 100.0).round() as i64;

    rsx! {
        div {
            // Sync
            div { class: "card",
                div { class: "section-title", "Sync" }
                h3 { style: "margin-bottom: 4px;",
                    "Sync server "
                    if has_token && health.session_expired() {
                        span { class: "badge fail", style: "margin-left: 8px;", "Session expired" }
                    } else if has_token {
                        span { class: "badge active", style: "margin-left: 8px;", "Connected" }
                    } else {
                        span { class: "badge", style: "margin-left: 8px;", "Not connected" }
                    }
                }
                p { class: "muted", style: "font-size: 13px; margin-bottom: 14px;",
                    "Enter the server URL and email. A one-time code is emailed for auth. "
                    Link { to: Route::Privacy {}, class: "link", "Privacy policy" }
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
                        button { class: "outline",
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
                        div { class: "row", style: "gap: 8px; align-items: baseline;",
                            span { class: "hint", "Check your email for the 6-digit code." }
                            button {
                                class: "link",
                                onclick: move |_| {
                                    crate::rt::kv_delete(OTP_PENDING_KEY);
                                    otp_code.set(String::new());
                                    otp_sent.set(false);
                                },
                                "Use a different email"
                            }
                        }
                    }
                }
                if let Some((msg, err)) = sync_status.read().clone() {
                    div { style: "margin-top: 10px;",
                        span { class: if err { "error" } else { "ok" }, "{msg}" }
                    }
                }
                if consent_missing {
                    hr { class: "divider" }
                    div { class: "row", style: "gap: 12px; align-items: center; flex-wrap: wrap;",
                        span { class: "error",
                            "Sync is paused: Yomeru needs your permission to send your email and sign-in data to the server."
                        }
                        button { class: "primary", onclick: on_allow_sync, "Allow" }
                    }
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
                    label { "Graduate when next review is ≥ N days away" }
                    input {
                        r#type: "number", min: "0",
                        value: "{cur.graduation_interval_days}",
                        oninput: move |e| settings.write().graduation_interval_days = e.value().parse().unwrap_or(0),
                    }
                    span { class: "hint", "0 = never graduate · e.g. 365 = 1 year" }
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
                    label { "Target retention {retention_pct}%" }
                    input {
                        r#type: "range", min: "0.70", max: "0.97", step: "0.01",
                        value: "{cur.request_retention}",
                        oninput: move |e| settings.write().request_retention = e.value().parse().unwrap_or(0.9),
                    }
                    span { class: "hint", "Desired recall probability. Higher = reviews come sooner." }
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

            // Account: last, away from everyday settings.
            if has_token || delete_status.read().is_some() {
                div { class: "card",
                    div { class: "section-title", "Account" }
                    h3 { style: "margin-bottom: 4px;", "Delete account" }
                    p { class: "muted", style: "font-size: 13px; margin-bottom: 14px;",
                        "Erases your cards, review history and settings from the sync server, for every device, after a code sent by email. The cards on this device are kept."
                    }
                    if has_token {
                        if *delete_pending.read() {
                            div { class: "form-row",
                                label { "Deletion code" }
                                div { class: "row", style: "gap: 8px; flex-wrap: wrap;",
                                    input {
                                        r#type: "text", inputmode: "numeric", maxlength: "6",
                                        placeholder: "000000",
                                        value: "{delete_code}",
                                        oninput: move |e| delete_code.set(e.value()),
                                        style: "max-width: 160px; letter-spacing: 0.3em; text-align: center;",
                                    }
                                    button { class: "danger",
                                        onclick: on_confirm_delete,
                                        disabled: *delete_busy.read() || delete_code.read().trim().len() != 6,
                                        if *delete_busy.read() { "Deleting…" } else { "Delete permanently" }
                                    }
                                    button { class: "outline",
                                        onclick: move |_| {
                                            delete_pending.set(false);
                                            delete_code.set(String::new());
                                            delete_status.set(None);
                                        },
                                        "Cancel"
                                    }
                                }
                                span { class: "hint", "This can't be undone." }
                            }
                        } else {
                            button { class: "danger",
                                onclick: on_delete_account,
                                disabled: *delete_busy.read(),
                                "Delete account"
                            }
                        }
                    }
                    if let Some((msg, err)) = delete_status.read().clone() {
                        div { style: "margin-top: 10px;",
                            span { class: if err { "error" } else { "ok" }, "{msg}" }
                        }
                    }
                }
            }
        }
    }
}

/// Drops the delta-sync cursor after a sign-in, so the next sync is a full
/// one: the session may belong to another account, and the cursor described
/// what this device had seen of the old one. Best effort — a failure here only
/// costs a delta that the server's epoch check may still turn into a full sync.
async fn forget_sync_cursor() {
    if let Err(e) = crate::idb::set_sync_cursor(None).await {
        dioxus::logger::tracing::warn!("forget sync cursor: {e}");
    }
}
