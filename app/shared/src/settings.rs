//! SrsSettings struct + free-function shims that route through the
//! [`crate::platform::SettingsStore`] in Dioxus context.
//!
//! The HTTP/localStorage implementation lives in `platform.rs`. Routes call
//! `load()` / `save()` exactly as before; the bytes underneath are owned by
//! whichever platform was installed via [`crate::launch_with`].

use crate::platform::Platform;
use dioxus::prelude::consume_context;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// localStorage / browser.storage.local key under which settings serialize.
pub const SETTINGS_KEY: &str = "srs_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsSettings {
    /// Card graduates once FSRS's computed next-review interval reaches this
    /// many days. 0 = never graduate.
    #[serde(rename = "graduationIntervalDays")]
    pub graduation_interval_days: u32,
    #[serde(rename = "intervalScale")]
    pub interval_scale: f64,
    #[serde(rename = "maxSessionCards")]
    pub max_session_cards: u32,
    /// FSRS desired retention (probability of recall at review time). Synced.
    /// Defaulted so settings stored before this field existed still load.
    #[serde(rename = "requestRetention", default = "default_request_retention")]
    pub request_retention: f64,
    #[serde(rename = "serverUrl")]
    pub server_url: String,
    #[serde(rename = "serverEmail")]
    pub server_email: String,
    #[serde(rename = "serverToken")]
    pub server_token: String,
    /// Wall-clock ms of the last *scheduler* edit on this device. Drives the
    /// last-write-wins merge of synced settings; never sent to the server as a
    /// stored field on its own, only inside the sync payload. Local-only fields
    /// (server_url/email/token) do not bump it.
    #[serde(rename = "settingsUpdatedMs", default)]
    pub settings_updated_ms: f64,
}

/// Serde default for [`SrsSettings::request_retention`]; single source of truth
/// is the SRS core constant.
fn default_request_retention() -> f64 {
    srs_core::DEFAULT_REQUEST_RETENTION
}

impl Default for SrsSettings {
    fn default() -> Self {
        Self {
            graduation_interval_days: 365,
            interval_scale: 1.0,
            max_session_cards: 20,
            request_retention: default_request_retention(),
            server_url: String::new(),
            server_email: String::new(),
            server_token: String::new(),
            settings_updated_ms: 0.0,
        }
    }
}

/// Synchronous read of the current settings off whichever store the
/// platform installed.
pub fn load() -> SrsSettings {
    consume_context::<Platform>().settings.load()
}

/// Saves settings. The save is fire-and-forget on
/// the underlying async store — callers that want backpressure should use
/// the trait directly.
pub fn save(s: &SrsSettings) -> Result<(), String> {
    let platform = consume_context::<Platform>();
    let s_owned = s.clone();
    crate::rt::spawn_detached(async move {
        if let Err(e) = platform.settings.save(s_owned).await {
            warn!("settings save failed: {e}");
        }
    });
    Ok(())
}

/// Default sync server URL.
/// * `YOMERU_SERVER_URL` at build time, when set and non-empty, wins
///   everywhere.
/// * Debug builds (`dx serve`): the local backend on :4500, so the dev loop
///   needs no manual config. The Android emulator reaches the host's loopback
///   as 10.0.2.2.
/// * Release web builds: the page origin, which is correct for self-hosters
///   who proxy /api/* through the same nginx that serves the SPA.
/// * Release native builds have no origin: empty, so the user enters a
///   server in Settings.
pub fn default_server_url() -> String {
    // Empty counts as unset: CI passes "" for a secret that isn't defined.
    if let Some(url) = option_env!("YOMERU_SERVER_URL").filter(|u| !u.is_empty()) {
        return url.to_string();
    }
    if cfg!(debug_assertions) {
        let host = if cfg!(target_os = "android") {
            "10.0.2.2"
        } else {
            "127.0.0.1"
        };
        return format!("http://{host}:4500");
    }
    page_origin()
}

#[cfg(target_arch = "wasm32")]
fn page_origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn page_origin() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The extension's `extension/src/shared/types.ts` defines an
    /// equivalent SrsSettings shape; both must round-trip the same JSON
    /// because they share `browser.storage.local`. If someone renames a
    /// field here without touching the TS counterpart, this test won't
    /// catch it — but a `serde_json::Value` snapshot of the field set will
    /// at least fail loudly if a *Rust*-side rename slips in.
    #[test]
    fn serializes_with_expected_field_names() {
        let s = SrsSettings::default();
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        for field in [
            "graduationIntervalDays",
            "intervalScale",
            "maxSessionCards",
            "requestRetention",
            "serverUrl",
            "serverEmail",
            "serverToken",
            "settingsUpdatedMs",
        ] {
            assert!(obj.contains_key(field), "missing field {field}");
        }
    }
}
