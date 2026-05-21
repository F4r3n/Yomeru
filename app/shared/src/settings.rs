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
    LocalStorage::get::<SrsSettings>(KEY).unwrap_or_default()
}

pub fn save(s: &SrsSettings) -> Result<(), gloo_storage::errors::StorageError> {
    LocalStorage::set(KEY, s)
}
