//! Target glue: the handful of host services the app needs — clock, random,
//! detached tasks, timers, a small key-value store, a confirm prompt and a
//! JSON POST — behind one API, so the rest of the crate never names a web or
//! native crate directly.
//!
//! * wasm32 (website, extension): `js_sys`, `localStorage`, `gloo-*`.
//! * native (Android, host tests): `SystemTime`, a JSON file in
//!   [`data_dir`], tokio, reqwest. Dioxus mobile runs Rust natively behind a
//!   WebView, where every `web_sys` call would panic.

use serde::Serialize;
use serde::de::DeserializeOwned;

pub use imp::*;

/// What a POST came back with. The body is read eagerly as text: every caller
/// either parses it as JSON or shows it in an error.
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_str(&self.body).map_err(|e| e.to_string())
    }
}

/// POSTs `body` as JSON, with a bearer token when given.
pub async fn post_json<B: Serialize + ?Sized>(
    url: &str,
    body: &B,
    bearer: Option<&str>,
) -> Result<HttpResponse, String> {
    let json = serde_json::to_string(body).map_err(|e| e.to_string())?;
    imp::post(url, json, bearer).await
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::HttpResponse;
    use gloo_storage::{LocalStorage, Storage};
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use std::future::Future;

    pub fn now_ms() -> f64 {
        js_sys::Date::now()
    }

    /// Uniform in `[0, 1)`.
    pub fn random() -> f64 {
        js_sys::Math::random()
    }

    /// Runs `fut` to completion independently of any component, so leaving
    /// a page can't cancel it.
    pub fn spawn_detached(fut: impl Future<Output = ()> + 'static) {
        wasm_bindgen_futures::spawn_local(fut);
    }

    pub async fn sleep_ms(ms: u32) {
        gloo_timers::future::TimeoutFuture::new(ms).await;
    }

    pub fn kv_get<T: DeserializeOwned>(key: &str) -> Option<T> {
        LocalStorage::get(key).ok()
    }

    pub fn kv_set<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
        LocalStorage::set(key, value).map_err(|e| e.to_string())
    }

    pub fn kv_delete(key: &str) {
        LocalStorage::delete(key);
    }

    pub async fn confirm(message: &str) -> bool {
        web_sys::window()
            .and_then(|w| w.confirm_with_message(message).ok())
            .unwrap_or(false)
    }

    pub(super) async fn post(
        url: &str,
        json: String,
        bearer: Option<&str>,
    ) -> Result<HttpResponse, String> {
        let mut req = gloo_net::http::Request::post(url).header("Content-Type", "application/json");
        if let Some(token) = bearer {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let res = req
            .body(json)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let body = res.text().await.map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, body })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::HttpResponse;
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn now_ms() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }

    /// Uniform in `[0, 1)`. Only shuffles review order, so a fast
    /// non-cryptographic generator is plenty.
    pub fn random() -> f64 {
        fastrand::f64()
    }

    /// Runs `fut` on the root scope, so it outlives the calling component.
    /// Must be called from inside the Dioxus runtime (event handlers, tasks).
    pub fn spawn_detached(fut: impl Future<Output = ()> + 'static) {
        dioxus::core::spawn_forever(fut);
    }

    pub async fn sleep_ms(ms: u32) {
        tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
    }

    /// Where the app keeps its files. On Android that's the app's private
    /// files dir, found from the package name in `/proc/self/cmdline` so no
    /// JNI is needed. Elsewhere (host tests, desktop runs) `YOMERU_DATA_DIR`
    /// or the XDG data dir.
    pub fn data_dir() -> &'static Path {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = find_data_dir();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("[yomeru] creating {}: {e}", dir.display());
            }
            dir
        })
    }

    fn find_data_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("YOMERU_DATA_DIR") {
            return PathBuf::from(dir);
        }
        if cfg!(target_os = "android")
            && let Ok(cmdline) = std::fs::read("/proc/self/cmdline")
        {
            // The process name is the package, possibly with a `:service`
            // suffix; argv[0] ends at the first NUL.
            let name = cmdline.split(|b| *b == 0).next().unwrap_or_default();
            let pkg = String::from_utf8_lossy(name);
            let pkg = pkg.split(':').next().unwrap_or_default();
            if !pkg.is_empty() {
                return PathBuf::from(format!("/data/data/{pkg}/files"));
            }
        }
        if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(dir).join("yomeru");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/yomeru");
        }
        std::env::temp_dir().join("yomeru")
    }

    // ── Key-value store ──────────────────────────────────────────────
    //
    // The native stand-in for localStorage: one JSON object in
    // `prefs.json`, loaded once and rewritten (via a temp file + rename, so a
    // crash can't leave it half-written) on every change. It only holds a
    // few small values — settings, theme, lookup history.

    type Prefs = serde_json::Map<String, serde_json::Value>;

    fn prefs() -> &'static Mutex<Prefs> {
        static PREFS: OnceLock<Mutex<Prefs>> = OnceLock::new();
        PREFS.get_or_init(|| {
            let path = data_dir().join("prefs.json");
            let map = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            Mutex::new(map)
        })
    }

    fn persist(map: &Prefs) -> Result<(), String> {
        let path = data_dir().join("prefs.json");
        let tmp = data_dir().join("prefs.json.tmp");
        let bytes = serde_json::to_vec(map).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    pub fn kv_get<T: DeserializeOwned>(key: &str) -> Option<T> {
        let map = prefs().lock().ok()?;
        serde_json::from_value(map.get(key)?.clone()).ok()
    }

    pub fn kv_set<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
        let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
        let mut map = prefs().lock().map_err(|e| e.to_string())?;
        map.insert(key.to_owned(), value);
        persist(&map)
    }

    pub fn kv_delete(key: &str) {
        let Ok(mut map) = prefs().lock() else {
            return;
        };
        if map.remove(key).is_some()
            && let Err(e) = persist(&map)
        {
            log::warn!("[yomeru] deleting pref {key}: {e}");
        }
    }

    /// The WebView's own `confirm()`. Dioxus mobile allows `eval`, unlike the
    /// extension's CSP.
    pub async fn confirm(message: &str) -> bool {
        let Ok(msg) = serde_json::to_string(message) else {
            return false;
        };
        let mut eval = dioxus::document::eval(&format!("dioxus.send(window.confirm({msg}));"));
        eval.recv::<bool>().await.unwrap_or(false)
    }

    fn client() -> &'static reqwest::Client {
        static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
        CLIENT.get_or_init(reqwest::Client::new)
    }

    pub(super) async fn post(
        url: &str,
        json: String,
        bearer: Option<&str>,
    ) -> Result<HttpResponse, String> {
        let mut req = client()
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(json);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let res = req.send().await.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let body = res.text().await.map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, body })
    }
}
