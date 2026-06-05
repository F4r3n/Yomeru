//! Platform abstraction: traits that swap between the web/android (HTTP-
//! backed) flavor of dict lookup + settings storage and an extension flavor
//! that messages a background script.
//!
//! The traits live here; routes call them through thin shims in
//! [`crate::dict`] and [`crate::settings`] that read a [`Platform`] out of
//! Dioxus context. Existing call sites in `routes/*` are unchanged — they
//! still see `dict::lookup(word)` etc.; only the implementations move.

use std::cell::RefCell;
use std::rc::Rc;

use examples_types::ExampleEntry;
use gloo_net::http::Request;
use gloo_storage::{LocalStorage, Storage};
use gloo_timers::callback::Timeout;
use jmdict_types::WordEntry;
use kanjidic_types::KanjiEntry;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

use crate::settings::{SETTINGS_KEY, SrsSettings, default_server_url};
use crate::types::SrsCard;
use async_trait::async_trait;
use dioxus::prelude::*;
/// Read-only dictionary surface. Web/android use [`DefaultPlatform`] which
/// posts JSON to the yomeru-server; the extension implements this against
/// `browser.runtime.sendMessage` so its in-WASM dicts service the request.
#[async_trait(?Send)]
pub trait DictClient {
    async fn lookup(&self, word: &str) -> Result<Vec<WordEntry>, String>;
    async fn lookup_many(&self, words: &[String]) -> Result<Vec<Vec<WordEntry>>, String>;
    async fn lookup_by_sequence(&self, sequences: &[u32])
    -> Result<Vec<Option<WordEntry>>, String>;
    async fn lookup_prefix(&self, text: &str, max: u8) -> Result<Vec<WordEntry>, String>;
    async fn kanji_for(&self, word: &str) -> Result<Vec<KanjiEntry>, String>;
    async fn examples_for(&self, word: &str, max: u8) -> Result<Vec<ExampleEntry>, String>;
}

/// Settings persistence + sync orchestration. `load()` is intentionally
/// synchronous so existing call sites (event handlers, derive-during-render
/// snippets) stay tight; implementations back it with a cached snapshot of
/// the actual store. Async [`load_async`] is for one-shot hydration at
/// startup.
///

#[async_trait(?Send)]
pub trait SettingsStore {
    /// Synchronous read off the in-process cache.
    fn load(&self) -> SrsSettings;
    /// Async refresh from the underlying store. Web/android tail this on
    /// startup; the extension uses it to re-hydrate when the background
    /// pushes a `storage.onChanged` event.
    async fn load_async(&self) -> Result<SrsSettings, String>;
    async fn save(&self, s: SrsSettings) -> Result<(), String>;
    /// Arm a debounced auto-sync. No-op in cores that have no server
    /// configured.
    fn schedule_sync(&self);
    /// Force an immediate sync, bypassing the debounce. Used by the "Sync
    /// now" button.
    async fn sync_now(&self) -> Result<String, String>;
    /// One-shot auth: ask the server to email an OTP. `Ok(Some(token))`
    /// means dev mode — server skipped the email and handed back a token.
    async fn request_otp(&self, server_url: &str, email: &str) -> Result<Option<String>, String>;
    async fn verify_otp(&self, server_url: &str, email: &str, code: &str)
    -> Result<String, String>;
}

/// What `routes/*` read out of Dioxus context. Cheap to clone — both fields
/// are `Rc`-counted trait objects.
#[derive(Clone)]
pub struct Platform {
    pub dict: Rc<dyn DictClient>,
    pub settings: Rc<dyn SettingsStore>,
}

// ── Default (HTTP) implementation ─────────────────────────────────────

/// The web + android flavor: dict and settings via HTTP / localStorage,
/// auto-sync via an in-process [`gloo_timers`] debouncer.
pub fn default_http_platform() -> Platform {
    Platform {
        dict: Rc::new(HttpDict),
        settings: Rc::new(LocalSettings::new()),
    }
}

// ---------- HttpDict ----------

struct HttpDict;

fn api_url(path: &str) -> String {
    let base = default_server_url();
    if base.is_empty() {
        return path.to_string();
    }
    format!("{}{}", base.trim_end_matches('/'), path)
}

#[derive(Serialize)]
struct LookupBody<'a> {
    words: &'a [String],
}
#[derive(Deserialize)]
struct LookupResponse {
    results: Vec<Vec<WordEntry>>,
}
#[derive(Serialize)]
struct LookupPrefixBody<'a> {
    text: &'a str,
    max: u8,
}
#[derive(Deserialize)]
struct LookupPrefixResponse {
    results: Vec<WordEntry>,
}
#[derive(Serialize)]
struct LookupBySequenceBody<'a> {
    sequences: &'a [u32],
}
#[derive(Deserialize)]
struct LookupBySequenceResponse {
    results: Vec<Option<WordEntry>>,
}
#[derive(Serialize)]
struct WordBody<'a> {
    word: &'a str,
}
#[derive(Serialize)]
struct WordMaxBody<'a> {
    word: &'a str,
    max: u8,
}
#[derive(Deserialize)]
struct KanjiResponse {
    entries: Vec<KanjiEntry>,
}
#[derive(Deserialize)]
struct ExamplesResponse {
    entries: Vec<ExampleEntry>,
}

async fn post_json<B: Serialize, R: for<'de> Deserialize<'de>>(
    path: &str,
    body: &B,
) -> Result<R, String> {
    let res = Request::post(&api_url(path))
        .json(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    res.json().await.map_err(|e| e.to_string())
}

#[async_trait(?Send)]
impl DictClient for HttpDict {
    async fn lookup(&self, word: &str) -> Result<Vec<WordEntry>, String> {
        let mut results = self.lookup_many(&[word.to_owned()]).await?;
        Ok(results.pop().unwrap_or_default())
    }

    async fn lookup_many(&self, words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
        let parsed: LookupResponse = post_json("/api/lookup", &LookupBody { words }).await?;
        Ok(parsed.results)
    }

    async fn lookup_by_sequence(
        &self,
        sequences: &[u32],
    ) -> Result<Vec<Option<WordEntry>>, String> {
        let parsed: LookupBySequenceResponse = post_json(
            "/api/lookup-by-sequence",
            &LookupBySequenceBody { sequences },
        )
        .await?;
        Ok(parsed.results)
    }

    async fn lookup_prefix(&self, text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
        let parsed: LookupPrefixResponse =
            post_json("/api/lookup-prefix", &LookupPrefixBody { text, max }).await?;
        Ok(parsed.results)
    }

    async fn kanji_for(&self, word: &str) -> Result<Vec<KanjiEntry>, String> {
        let parsed: KanjiResponse = post_json("/api/kanji", &WordBody { word }).await?;
        Ok(parsed.entries)
    }

    async fn examples_for(&self, word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
        let parsed: ExamplesResponse =
            post_json("/api/examples", &WordMaxBody { word, max }).await?;
        Ok(parsed.entries)
    }
}

// ---------- LocalSettings ----------

/// Settings backed by `window.localStorage`, sync scheduling by an in-
/// process [`gloo_timers::callback::Timeout`].
struct LocalSettings {
    state: Rc<RefCell<SyncState>>,
}

#[derive(Default)]
struct SyncState {
    pending: Option<Timeout>,
    in_flight: bool,
    retry: bool,
}

const DEBOUNCE_MS: u32 = 2_000;

impl LocalSettings {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(SyncState::default())),
        }
    }
}

#[derive(Serialize)]
struct AuthRequestBody<'a> {
    email: &'a str,
}

#[derive(Serialize)]
struct VerifyBody<'a> {
    email: &'a str,
    code: &'a str,
}

#[derive(Deserialize)]
struct VerifyResponse {
    token: String,
}

/// Synced scheduler settings, on the wire to/from the server. Field names are
/// snake_case to match the server's `db::Settings`. Local-only connection
/// fields (server_url/email/token) are deliberately absent.
#[derive(Serialize, Deserialize, Clone)]
struct SettingsPayload {
    graduation_reps: u32,
    interval_scale: f64,
    max_session_cards: u32,
    request_retention: f64,
    updated_ms: f64,
}

impl SettingsPayload {
    fn from_settings(s: &SrsSettings) -> Self {
        Self {
            graduation_reps: s.graduation_reps,
            interval_scale: s.interval_scale,
            max_session_cards: s.max_session_cards,
            request_retention: s.request_retention,
            updated_ms: s.settings_updated_ms,
        }
    }
}

#[derive(Serialize)]
struct SyncBody<'a> {
    cards: &'a [SrsCard],
    deletions: &'a [String],
    settings: SettingsPayload,
}

#[derive(Deserialize, Default)]
struct SyncResponse {
    cards: Vec<SrsCard>,
    #[serde(default)]
    deletions: Vec<String>,
    #[serde(default)]
    settings: Option<SettingsPayload>,
}

fn join_url(base: &str, path: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}{path}")
}

async fn do_sync(state: Rc<RefCell<SyncState>>) -> Result<String, String> {
    use crate::idb::{apply_remote_deletions, clear_tombstones, get_all_cards, get_all_tombstones};

    let s: SrsSettings = LocalStorage::get(SETTINGS_KEY).unwrap_or_default();
    if s.server_url.is_empty() || s.server_token.is_empty() {
        return Err("not authenticated".into());
    }
    let local_cards = get_all_cards()
        .await
        .map_err(|e| format!("read cards: {e}"))?;
    let local_tombstones = get_all_tombstones()
        .await
        .map_err(|e| format!("read tombstones: {e}"))?;

    let res = Request::post(&join_url(s.server_url.trim(), "/api/sync"))
        .header("Authorization", &format!("Bearer {}", s.server_token))
        .json(&SyncBody {
            cards: &local_cards,
            deletions: &local_tombstones,
            settings: SettingsPayload::from_settings(&s),
        })
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status() == 401 {
        return Err("session expired — re-verify".into());
    }
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    let resp: SyncResponse = res.json().await.map_err(|e| e.to_string())?;

    if !resp.cards.is_empty() {
        put_cards_skip_older(&resp.cards).await?;
    }
    // Race-safe: ids we sent tombstones for must NOT be re-deleted —
    // either the local cards store already lacks them, or the user
    // re-added the card mid-sync and we'd silently eat the re-add.
    let sent: std::collections::HashSet<&str> =
        local_tombstones.iter().map(String::as_str).collect();
    let foreign: Vec<String> = resp
        .deletions
        .iter()
        .filter(|id| !sent.contains(id.as_str()))
        .cloned()
        .collect();
    apply_remote_deletions(&foreign)
        .await
        .map_err(|e| format!("apply deletions: {e}"))?;
    clear_tombstones(&local_tombstones)
        .await
        .map_err(|e| format!("clear tombstones: {e}"))?;

    // Adopt server-side settings if they're newer. Re-read from localStorage
    // (not the start-of-sync snapshot) so a scheduler edit made while the sync
    // was in flight isn't clobbered, and preserve the device-local connection
    // fields the server never sees.
    if let Some(remote) = resp.settings {
        let mut merged: SrsSettings = LocalStorage::get(SETTINGS_KEY).unwrap_or_default();
        if remote.updated_ms > merged.settings_updated_ms {
            merged.graduation_reps = remote.graduation_reps;
            merged.interval_scale = remote.interval_scale;
            merged.max_session_cards = remote.max_session_cards;
            merged.request_retention = remote.request_retention;
            merged.settings_updated_ms = remote.updated_ms;
            LocalStorage::set(SETTINGS_KEY, merged).map_err(|e| e.to_string())?;
        }
    }

    let _ = state; // borrowed by the runner above; nothing more to do here
    Ok(format!(
        "Synced {} card{}.",
        resp.cards.len(),
        if resp.cards.len() == 1 { "" } else { "s" }
    ))
}

/// Mirrors the server-side `last_review_ms` last-write-wins rule on the
/// client: skip an incoming card if the local copy has a newer review
/// timestamp (we reviewed it locally while the sync was in flight).
async fn put_cards_skip_older(remote: &[SrsCard]) -> Result<(), String> {
    use crate::idb::{get_all_cards, put_cards};
    let local = get_all_cards()
        .await
        .map_err(|e| format!("read cards for merge: {e}"))?;
    let local_by_id: std::collections::HashMap<&str, f64> = local
        .iter()
        .map(|c| (c.id.as_str(), c.last_review_ms.unwrap_or(0.0)))
        .collect();
    let to_put: Vec<SrsCard> = remote
        .iter()
        .filter(|c| {
            let local_ts = local_by_id.get(c.id.as_str()).copied().unwrap_or(0.0);
            c.last_review_ms.unwrap_or(0.0) >= local_ts
        })
        .cloned()
        .collect();
    put_cards(&to_put)
        .await
        .map_err(|e| format!("put cards: {e}"))
}

#[async_trait(?Send)]
impl SettingsStore for LocalSettings {
    fn load(&self) -> SrsSettings {
        let mut s: SrsSettings = LocalStorage::get(SETTINGS_KEY).unwrap_or_default();
        if s.server_url.is_empty() {
            s.server_url = default_server_url();
        }
        s
    }

    async fn load_async(&self) -> Result<SrsSettings, String> {
        Ok(self.load())
    }

    async fn save(&self, mut s: SrsSettings) -> Result<(), String> {
        // Bump the LWW merge key only when a *synced* scheduler field actually
        // changed. Saving the server URL/email/token (a different, device-local
        // concern) must not let a stale scheduler config win a later sync.
        let prev: SrsSettings = LocalStorage::get(SETTINGS_KEY).unwrap_or_default();
        let scheduler_changed = prev.graduation_reps != s.graduation_reps
            || prev.interval_scale != s.interval_scale
            || prev.max_session_cards != s.max_session_cards
            || prev.request_retention != s.request_retention;
        if scheduler_changed {
            s.settings_updated_ms = js_sys::Date::now();
        }
        LocalStorage::set(SETTINGS_KEY, s).map_err(|e| e.to_string())
    }

    fn schedule_sync(&self) {
        // Don't even arm the timer if we have nothing configured.
        let s = self.load();
        if s.server_url.is_empty() || s.server_token.is_empty() {
            return;
        }
        if self.state.borrow().in_flight {
            self.state.borrow_mut().retry = true;
            return;
        }
        let state = self.state.clone();
        let t = Timeout::new(DEBOUNCE_MS, move || {
            state.borrow_mut().pending = None;
            let state = state.clone();
            spawn_local(async move {
                state.borrow_mut().in_flight = true;
                if let Err(e) = do_sync(state.clone()).await {
                    warn!("[yomeru] auto-sync failed: {e}");
                }
                state.borrow_mut().in_flight = false;
                if std::mem::replace(&mut state.borrow_mut().retry, false) {
                    // Recursively re-arm — same path as `schedule_sync`, but
                    // we can't call &self from inside spawn_local, so inline.
                    let st = state.clone();
                    let t = Timeout::new(DEBOUNCE_MS, move || {
                        st.borrow_mut().pending = None;
                        let st = st.clone();
                        spawn_local(async move {
                            st.borrow_mut().in_flight = true;
                            if let Err(e) = do_sync(st.clone()).await {
                                warn!("[yomeru] auto-sync retry failed: {e}");
                            }
                            st.borrow_mut().in_flight = false;
                        });
                    });
                    state.borrow_mut().pending = Some(t);
                }
            });
        });
        self.state.borrow_mut().pending = Some(t);
    }

    async fn sync_now(&self) -> Result<String, String> {
        // Cancel any pending debounce — we're about to do the work now.
        if let Some(t) = self.state.borrow_mut().pending.take() {
            t.cancel();
        }
        if self.state.borrow().in_flight {
            self.state.borrow_mut().retry = true;
            return Ok("Sync already in progress — will repeat when it finishes.".into());
        }
        self.state.borrow_mut().in_flight = true;
        let result = do_sync(self.state.clone()).await;
        self.state.borrow_mut().in_flight = false;
        if std::mem::replace(&mut self.state.borrow_mut().retry, false) {
            self.schedule_sync();
        }
        result
    }

    async fn request_otp(&self, server_url: &str, email: &str) -> Result<Option<String>, String> {
        let res = Request::post(&join_url(server_url, "/api/auth/request"))
            .json(&AuthRequestBody { email })
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !res.ok() {
            return Err(format!("server {}", res.status()));
        }
        if res.status() == 204 {
            return Ok(None);
        }
        let parsed: VerifyResponse = res.json().await.map_err(|e| e.to_string())?;
        Ok(Some(parsed.token))
    }

    async fn verify_otp(
        &self,
        server_url: &str,
        email: &str,
        code: &str,
    ) -> Result<String, String> {
        let res = Request::post(&join_url(server_url, "/api/auth/verify"))
            .json(&VerifyBody { email, code })
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !res.ok() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("server {status}: {text}"));
        }
        let parsed: VerifyResponse = res.json().await.map_err(|e| e.to_string())?;
        Ok(parsed.token)
    }
}
