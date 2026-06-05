use dioxus::prelude::*;
use gloo_storage::{LocalStorage, Storage};
use serde_json::json;
use wasm_bindgen::JsCast;

use crate::api;
use crate::dict::lookup_many;
use crate::idb::{get_all_cards, put_cards};
use crate::settings::{load, save};
use crate::sync::{schedule_sync, sync_now};
use crate::types::{CARDS_SCHEMA_VERSION, SrsCard, SrsCardV1};

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
        let pending: String = LocalStorage::get(OTP_PENDING_KEY).unwrap_or_default();
        !pending.is_empty() && pending == load().server_email.trim()
    });
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
                    settings.set(next);
                    LocalStorage::delete(OTP_PENDING_KEY);
                    otp_sent.set(false);
                    sync_status.set(Some((
                        "Authenticated (dev mode — no code required).".into(),
                        false,
                    )));
                }
                Ok(None) => {
                    let _ = LocalStorage::set(OTP_PENDING_KEY, s.server_email.trim());
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
                    settings.set(next);
                    LocalStorage::delete(OTP_PENDING_KEY);
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
                "schema": CARDS_SCHEMA_VERSION,
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
    let saved_flag = *saved.read();
    let retention_pct = (cur.request_retention * 100.0).round() as i64;

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "Settings" }
                    div { class: "subtitle", "Tune the scheduler, back up cards, and configure sync." }
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
                        div { class: "row", style: "gap: 8px; align-items: baseline;",
                            span { class: "hint", "Check your email for the 6-digit code." }
                            button {
                                class: "link",
                                onclick: move |_| {
                                    LocalStorage::delete(OTP_PENDING_KEY);
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

/// Breakdown of cards that were not imported, by reason.
#[derive(Default, Clone, Copy)]
struct Skips {
    /// Id already present in storage.
    existing: usize,
    /// Legacy headword not found in the dictionary (no `sequence` to key by).
    unresolved: usize,
    /// Could not be parsed in the expected format.
    invalid: usize,
}

impl Skips {
    fn total(self) -> usize {
        self.existing + self.unresolved + self.invalid
    }
}

/// Human-readable import summary, e.g.
/// "Imported 3 card(s); skipped 160 (157 already present, 3 not in dictionary)."
fn format_import_result(added: usize, skips: Skips) -> String {
    if skips.total() == 0 {
        return format!("Imported {added} card(s).");
    }
    let mut reasons = Vec::new();
    if skips.existing > 0 {
        reasons.push(format!("{} already present", skips.existing));
    }
    if skips.unresolved > 0 {
        reasons.push(format!("{} not in dictionary", skips.unresolved));
    }
    if skips.invalid > 0 {
        reasons.push(format!("{} invalid", skips.invalid));
    }
    format!(
        "Imported {added} card(s); skipped {} ({}).",
        skips.total(),
        reasons.join(", ")
    )
}

async fn import_cards_json(text: &str) -> Result<(usize, Skips), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let arr = v
        .get("cards")
        .and_then(|c| c.as_array())
        .ok_or("missing 'cards' array")?;

    let existing = get_all_cards().await.map_err(|e| e.to_string())?;
    let existing_ids: std::collections::HashSet<String> =
        existing.into_iter().map(|c| c.id).collect();

    // Route by the explicit `schema` version. Legacy exports predate this field
    // (or carry a lower number) and keyed cards by the headword string rather
    // than the JMdict `sequence`. Note: the human-facing `version` field cannot
    // discriminate — it's the crate version and is identical across formats.
    let schema = v.get("schema").and_then(|s| s.as_u64()).unwrap_or(1);
    let (to_put, skips) = if schema >= CARDS_SCHEMA_VERSION {
        parse_current_cards(arr, &existing_ids)
    } else {
        let word_seq = resolve_legacy_words(arr).await?;
        parse_legacy_cards(arr, &word_seq, &existing_ids)
    };

    let added = to_put.len();
    put_cards(&to_put).await.map_err(|e| e.to_string())?;
    if added > 0 {
        schedule_sync();
    }
    Ok((added, skips))
}

/// Parse current-format cards, which deserialize straight into [`SrsCard`].
/// Unparseable entries and ids already in `existing_ids` count as skipped.
fn parse_current_cards(
    cards: &[serde_json::Value],
    existing_ids: &std::collections::HashSet<String>,
) -> (Vec<SrsCard>, Skips) {
    let mut to_put = Vec::new();
    let mut skips = Skips::default();
    for c in cards {
        let Ok(card) = serde_json::from_value::<SrsCard>(c.clone()) else {
            skips.invalid += 1;
            continue;
        };
        if existing_ids.contains(&card.id) {
            skips.existing += 1;
            continue;
        }
        to_put.push(card);
    }
    (to_put, skips)
}

/// Resolve every legacy card's headword to its JMdict `sequence` in one batch.
async fn resolve_legacy_words(
    cards: &[serde_json::Value],
) -> Result<std::collections::HashMap<String, u32>, String> {
    let mut words: Vec<String> = Vec::new();
    for c in cards {
        if let Ok(v1) = serde_json::from_value::<SrsCardV1>(c.clone())
            && !words.contains(&v1.word)
        {
            words.push(v1.word);
        }
    }
    let mut word_seq = std::collections::HashMap::new();
    if !words.is_empty() {
        let results = lookup_many(&words).await.map_err(|e| e.to_string())?;
        for (w, entries) in words.into_iter().zip(results) {
            if let Some(seq) = entries.first().map(|e| e.sequence) {
                word_seq.insert(w, seq);
            }
        }
    }
    Ok(word_seq)
}

/// Parse legacy-format cards into [`SrsCardV1`] and upgrade each to the current
/// [`SrsCard`] using the resolved `word_seq`. Cards whose word is absent from
/// `word_seq` (not in the dictionary), already-present ids, and unparseable
/// entries are tallied by reason. Returns `(to_put, skips)`.
fn parse_legacy_cards(
    cards: &[serde_json::Value],
    word_seq: &std::collections::HashMap<String, u32>,
    existing_ids: &std::collections::HashSet<String>,
) -> (Vec<SrsCard>, Skips) {
    let mut to_put = Vec::new();
    let mut skips = Skips::default();
    for c in cards {
        let Ok(v1) = serde_json::from_value::<SrsCardV1>(c.clone()) else {
            skips.invalid += 1;
            continue;
        };
        let Some(&seq) = word_seq.get(&v1.word) else {
            skips.unresolved += 1;
            continue;
        };
        let card = v1.upgrade(seq);
        if existing_ids.contains(&card.id) {
            skips.existing += 1;
            continue;
        }
        to_put.push(card);
    }
    (to_put, skips)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CardDirection;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};

    /// A new-format card: keyed by numeric `sequence`, id is `{seq}::{dir}`.
    fn new_format_card(sequence: u32, direction: &str) -> serde_json::Value {
        json!({
            "id": format!("{sequence}::{direction}"),
            "sequence": sequence,
            "direction": direction,
            "due_ms": 1.0,
            "stability": 2.0,
            "difficulty": 3.0,
            "reps": 4,
            "lapses": 0,
            "state": "review",
            "last_review_ms": 0.5,
            "added_ms": 0.0,
            "status": "active",
        })
    }

    /// An old-format card: keyed by `word`, no `sequence`, id is `{word}::{dir}`.
    fn old_format_card(word: &str, direction: &str) -> serde_json::Value {
        json!({
            "id": format!("{word}::{direction}"),
            "word": word,
            "direction": direction,
            "due_ms": 1.0,
            "stability": 2.0,
            "difficulty": 3.0,
            "reps": 4,
            "lapses": 0,
            "state": "review",
            "last_review_ms": 0.5,
            "added_ms": 0.0,
            "status": "active",
        })
    }

    // --- current format -----------------------------------------------------

    #[test]
    fn current_card_imports_unchanged() {
        let cards = vec![new_format_card(1001, "recall")];
        let (to_put, skips) = parse_current_cards(&cards, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put.len(), 1);
        assert_eq!(to_put[0].sequence, 1001);
        assert_eq!(to_put[0].id, "1001::recall");
    }

    #[test]
    fn current_existing_ids_are_skipped() {
        let cards = vec![new_format_card(1001, "recall"), new_format_card(1002, "recall")];
        let existing = HashSet::from(["1001::recall".to_string()]);
        let (to_put, skips) = parse_current_cards(&cards, &existing);
        assert_eq!(skips.existing, 1);
        assert_eq!(skips.total(), 1);
        assert_eq!(to_put.len(), 1);
        assert_eq!(to_put[0].sequence, 1002);
    }

    #[test]
    fn current_unparseable_card_is_invalid() {
        // Missing the required `sequence` field => unparseable as SrsCard.
        let cards = vec![json!({ "id": "x", "direction": "recall" })];
        let (to_put, skips) = parse_current_cards(&cards, &HashSet::new());
        assert_eq!(skips.invalid, 1);
        assert!(to_put.is_empty());
    }

    // --- legacy (v1) format --------------------------------------------------

    #[test]
    fn legacy_card_resolves_and_recanonicalizes_id() {
        let cards = vec![old_format_card("一発", "recall")];
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        let (to_put, skips) = parse_legacy_cards(&cards, &word_seq, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put.len(), 1);
        let card = &to_put[0];
        assert_eq!(card.sequence, 1583460);
        // id is rebuilt from sequence, not the legacy `word::dir` form.
        assert_eq!(card.id, "1583460::recall");
        assert_eq!(card.direction, CardDirection::Recall);
        // Other fields survive the upgrade.
        assert_eq!(card.stability, 2.0);
        assert_eq!(card.last_review_ms, Some(0.5));
    }

    #[test]
    fn legacy_card_unresolved_when_word_not_in_dictionary() {
        let cards = vec![old_format_card("珍しい単語", "recall")];
        let (to_put, skips) = parse_legacy_cards(&cards, &HashMap::new(), &HashSet::new());
        assert_eq!(skips.unresolved, 1);
        assert!(to_put.is_empty());
    }

    #[test]
    fn legacy_existing_ids_are_skipped() {
        let cards = vec![old_format_card("一発", "recall")];
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        // The *rebuilt* (sequence-keyed) id already exists.
        let existing = HashSet::from(["1583460::recall".to_string()]);
        let (to_put, skips) = parse_legacy_cards(&cards, &word_seq, &existing);
        assert_eq!(skips.existing, 1);
        assert!(to_put.is_empty());
    }

    #[test]
    fn legacy_batch_counts_by_reason() {
        let cards = vec![
            old_format_card("一発", "recall"),  // resolved -> imported
            old_format_card("謎", "recall"),     // unresolved -> skipped
            json!({ "not": "a card" }),           // invalid -> skipped
        ];
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        let (to_put, skips) = parse_legacy_cards(&cards, &word_seq, &HashSet::new());
        assert_eq!(to_put.len(), 1);
        assert_eq!(skips.unresolved, 1);
        assert_eq!(skips.invalid, 1);
        assert_eq!(skips.total(), 2);
    }

    #[test]
    fn legacy_last_review_ms_optional() {
        // A v1 card with no last_review_ms still parses (field defaults to None).
        let mut card = old_format_card("一発", "recall");
        card.as_object_mut().unwrap().remove("last_review_ms");
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        let (to_put, skips) = parse_legacy_cards(&[card], &word_seq, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put[0].last_review_ms, None);
    }

    // --- result message ------------------------------------------------------

    #[test]
    fn message_all_imported() {
        assert_eq!(format_import_result(3, Skips::default()), "Imported 3 card(s).");
    }

    #[test]
    fn message_breaks_down_skips() {
        let skips = Skips { existing: 157, unresolved: 3, invalid: 0 };
        assert_eq!(
            format_import_result(0, skips),
            "Imported 0 card(s); skipped 160 (157 already present, 3 not in dictionary)."
        );
    }
}
