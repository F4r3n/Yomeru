use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};

const KEY: &str = "srs_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsSettings {
    #[serde(rename = "graduationReps")]
    pub graduation_reps: u32,
    #[serde(rename = "intervalScale")]
    pub interval_scale: f64,
    #[serde(rename = "maxSessionCards")]
    pub max_session_cards: u32,
    #[serde(rename = "serverUrl")]
    pub server_url: String,
    #[serde(rename = "serverEmail")]
    pub server_email: String,
    #[serde(rename = "serverToken")]
    pub server_token: String,
}

impl Default for SrsSettings {
    fn default() -> Self {
        Self {
            graduation_reps: 0,
            interval_scale: 1.0,
            max_session_cards: 20,
            server_url: String::new(),
            server_email: String::new(),
            server_token: String::new(),
        }
    }
}

pub fn load() -> SrsSettings {
    let mut s: SrsSettings = LocalStorage::get(KEY).unwrap_or_default();
    if s.server_url.is_empty() {
        s.server_url = default_server_url();
    }
    s
}

/// Default sync server URL.
/// * Debug builds (`dx serve`): point at the local backend on :4500 so the
///   dev loop doesn't need any manual config in Settings.
/// * Release builds (`dx bundle --release`): use the page origin, which is
///   correct for self-hosters who proxy /api/* through the same nginx that
///   serves the SPA.
pub fn default_server_url() -> String {
    if cfg!(debug_assertions) {
        return "http://127.0.0.1:4500".to_string();
    }
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default()
}

pub fn save(s: &SrsSettings) -> Result<(), gloo_storage::errors::StorageError> {
    LocalStorage::set(KEY, s)
}
