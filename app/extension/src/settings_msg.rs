use std::cell::RefCell;

use js_sys::Reflect;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use yomeru_shared::platform::{BoxFuture, SettingsStore};
use yomeru_shared::settings::SrsSettings;

use crate::bridge;

thread_local! {
    static CACHE: RefCell<SrsSettings> = RefCell::new(SrsSettings::default());
}

// ── Public init helpers ────────────────────────────────────────────────

pub async fn hydrate() -> Result<(), String> {
    let s = read_from_storage().await?;
    CACHE.with(|c| *c.borrow_mut() = s);
    Ok(())
}

pub fn register_storage_watcher() {
    let cb = Closure::<dyn Fn(JsValue)>::new(move |changes: JsValue| {
        let Ok(entry) = Reflect::get(&changes, &JsValue::from_str("srs_settings")) else {
            return;
        };
        if entry.is_undefined() {
            return;
        }
        let Ok(new_val) = Reflect::get(&entry, &JsValue::from_str("newValue")) else {
            return;
        };
        if new_val.is_undefined() || new_val.is_null() {
            return;
        }
        if let Ok(s) = serde_wasm_bindgen::from_value::<SrsSettings>(new_val) {
            CACHE.with(|c| *c.borrow_mut() = s);
        }
    });
    bridge::add_storage_listener(cb.as_ref().unchecked_ref());
    cb.forget(); // permanent listener
}

// ── Storage read helper ────────────────────────────────────────────────

async fn read_from_storage() -> Result<SrsSettings, String> {
    let js_val = JsFuture::from(bridge::storage_get("srs_settings"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let inner = Reflect::get(&js_val, &JsValue::from_str("srs_settings"))
        .map_err(|e| format!("{e:?}"))?;
    if inner.is_undefined() || inner.is_null() {
        return Ok(SrsSettings::default());
    }
    serde_wasm_bindgen::from_value(inner).map_err(|e| e.to_string())
}

// ── Response shapes ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SuccessResp {
    error: Option<String>,
}

#[derive(Deserialize)]
struct SyncResp {
    synced: Option<u32>,
    #[serde(default)]
    queued: bool,
    error: Option<String>,
}

#[derive(Deserialize)]
struct OtpRequestResp {
    #[serde(default)]
    token: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct OtpVerifyResp {
    token: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct OkResp {}

// ── SettingsStore impl ─────────────────────────────────────────────────

pub struct ExtensionSettings;

impl SettingsStore for ExtensionSettings {
    fn load(&self) -> SrsSettings {
        CACHE.with(|c| c.borrow().clone())
    }

    fn load_async<'a>(&'a self) -> BoxFuture<'a, Result<SrsSettings, String>> {
        Box::pin(async move {
            let s = read_from_storage().await?;
            CACHE.with(|c| *c.borrow_mut() = s.clone());
            Ok(s)
        })
    }

    fn save<'a>(&'a self, s: SrsSettings) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            // Optimistically update the local cache before the round-trip.
            CACHE.with(|c| *c.borrow_mut() = s.clone());
            let resp: SuccessResp = crate::send_bg_message("SAVE_SETTINGS", &s).await?;
            if let Some(e) = resp.error {
                return Err(e);
            }
            Ok(())
        })
    }

    fn schedule_sync(&self) {
        wasm_bindgen_futures::spawn_local(async {
            if let Err(e) = crate::send_bg_message::<_, OkResp>("BUMP_DB_VERSION", ()).await {
                log::warn!("schedule_sync send failed: {e}");
            }
        });
    }

    fn sync_now<'a>(&'a self) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let resp: SyncResp = crate::send_bg_message("SYNC_CARDS", ()).await?;
            if let Some(e) = resp.error {
                return Err(e);
            }
            if resp.queued {
                return Ok("Sync already in progress — will repeat when it finishes.".into());
            }
            let n = resp.synced.unwrap_or(0);
            Ok(format!("Synced {} card{}.", n, if n == 1 { "" } else { "s" }))
        })
    }

    fn request_otp<'a>(
        &'a self,
        server_url: &'a str,
        email: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        #[derive(Serialize)]
        struct Payload<'a> {
            #[serde(rename = "serverUrl")]
            server_url: &'a str,
            email: &'a str,
        }
        Box::pin(async move {
            let resp: OtpRequestResp =
                crate::send_bg_message("REQUEST_OTP", Payload { server_url, email }).await?;
            if let Some(e) = resp.error {
                return Err(e);
            }
            // None = OTP sent to email; Some(token) = dev-mode instant auth
            Ok(resp.token)
        })
    }

    fn verify_otp<'a>(
        &'a self,
        server_url: &'a str,
        email: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<String, String>> {
        #[derive(Serialize)]
        struct Payload<'a> {
            #[serde(rename = "serverUrl")]
            server_url: &'a str,
            email: &'a str,
            code: &'a str,
        }
        Box::pin(async move {
            let resp: OtpVerifyResp =
                crate::send_bg_message("VERIFY_OTP", Payload { server_url, email, code })
                    .await?;
            if let Some(e) = resp.error {
                return Err(e);
            }
            resp.token
                .ok_or_else(|| "verify_otp: background did not return a token".into())
        })
    }
}
