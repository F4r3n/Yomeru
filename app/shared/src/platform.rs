//! Platform abstraction: traits that swap between the web/android (HTTP-
//! backed) flavor of dict lookup + settings storage and an extension flavor
//! that messages a background script.
//!
//! The traits live here; routes call them through thin shims in
//! [`crate::dict`] and [`crate::settings`] that read a [`Platform`] out of
//! Dioxus context. Existing call sites in `routes/*` are unchanged — they
//! still see `dict::lookup(word)` etc.; only the implementations move.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use examples_types::ExampleEntry;
use gloo_net::http::Request;
use gloo_storage::{LocalStorage, Storage};
use gloo_timers::callback::Timeout;
use jmdict_types::WordEntry;
use kanjidic_types::KanjiEntry;
use log::warn;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

use crate::settings::{default_server_url, SrsSettings, SETTINGS_KEY};
use crate::types::SrsCard;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Read-only dictionary surface. Web/android use [`DefaultPlatform`] which
/// posts JSON to the yomeru-server; the extension implements this against
/// `browser.runtime.sendMessage` so its in-WASM dicts service the request.
pub trait DictClient {
    fn lookup<'a>(
        &'a self,
        word: &'a str,
    ) -> BoxFuture<'a, Result<Vec<WordEntry>, String>>;
    fn lookup_many<'a>(
        &'a self,
        words: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<WordEntry>>, String>>;
    fn lookup_prefix<'a>(
        &'a self,
        text: &'a str,
        max: u8,
    ) -> BoxFuture<'a, Result<Vec<WordEntry>, String>>;
    fn kanji_for<'a>(
        &'a self,
        word: &'a str,
    ) -> BoxFuture<'a, Result<Vec<KanjiEntry>, String>>;
    fn examples_for<'a>(
        &'a self,
        word: &'a str,
        max: u8,
    ) -> BoxFuture<'a, Result<Vec<ExampleEntry>, String>>;
}

/// Settings persistence + sync orchestration. `load()` is intentionally
/// synchronous so existing call sites (event handlers, derive-during-render
/// snippets) stay tight; implementations back it with a cached snapshot of
/// the actual store. Async [`load_async`] is for one-shot hydration at
/// startup.
pub trait SettingsStore {
    /// Synchronous read off the in-process cache.
    fn load(&self) -> SrsSettings;
    /// Async refresh from the underlying store. Web/android tail this on
    /// startup; the extension uses it to re-hydrate when the background
    /// pushes a `storage.onChanged` event.
    fn load_async<'a>(&'a self) -> BoxFuture<'a, Result<SrsSettings, String>>;
    fn save<'a>(
        &'a self,
        s: SrsSettings,
    ) -> BoxFuture<'a, Result<(), String>>;
    /// Arm a debounced auto-sync. No-op in cores that have no server
    /// configured.
    fn schedule_sync(&self);
    /// Force an immediate sync, bypassing the debounce. Used by the "Sync
    /// now" button.
    fn sync_now<'a>(&'a self) -> BoxFuture<'a, Result<String, String>>;
    /// One-shot auth: ask the server to email an OTP. `Ok(Some(token))`
    /// means dev mode — server skipped the email and handed back a token.
    fn request_otp<'a>(
        &'a self,
        server_url: &'a str,
        email: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, String>>;
    fn verify_otp<'a>(
        &'a self,
        server_url: &'a str,
        email: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<String, String>>;
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

impl DictClient for HttpDict {
    fn lookup<'a>(
        &'a self,
        word: &'a str,
    ) -> BoxFuture<'a, Result<Vec<WordEntry>, String>> {
        Box::pin(async move {
            let mut results = self.lookup_many(&[word.to_owned()]).await?;
            Ok(results.pop().unwrap_or_default())
        })
    }

    fn lookup_many<'a>(
        &'a self,
        words: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<WordEntry>>, String>> {
        Box::pin(async move {
            let parsed: LookupResponse =
                post_json("/api/lookup", &LookupBody { words }).await?;
            Ok(parsed.results)
        })
    }

    fn lookup_prefix<'a>(
        &'a self,
        text: &'a str,
        max: u8,
    ) -> BoxFuture<'a, Result<Vec<WordEntry>, String>> {
        Box::pin(async move {
            let parsed: LookupPrefixResponse =
                post_json("/api/lookup-prefix", &LookupPrefixBody { text, max }).await?;
            Ok(parsed.results)
        })
    }

    fn kanji_for<'a>(
        &'a self,
        word: &'a str,
    ) -> BoxFuture<'a, Result<Vec<KanjiEntry>, String>> {
        Box::pin(async move {
            let parsed: KanjiResponse =
                post_json("/api/kanji", &WordBody { word }).await?;
            Ok(parsed.entries)
        })
    }

    fn examples_for<'a>(
        &'a self,
        word: &'a str,
        max: u8,
    ) -> BoxFuture<'a, Result<Vec<ExampleEntry>, String>> {
        Box::pin(async move {
            let parsed: ExamplesResponse =
                post_json("/api/examples", &WordMaxBody { word, max }).await?;
            Ok(parsed.entries)
        })
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

#[derive(Serialize)]
struct SyncBody<'a> {
    cards: &'a [SrsCard],
    deletions: &'a [String],
}

#[derive(Deserialize, Default)]
struct SyncResponse {
    cards: Vec<SrsCard>,
    #[serde(default)]
    deletions: Vec<String>,
}

fn join_url(base: &str, path: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}{path}")
}

async fn do_sync(state: Rc<RefCell<SyncState>>) -> Result<String, String> {
    use crate::idb::{
        apply_remote_deletions, clear_tombstones, get_all_cards, get_all_tombstones,
    };

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

impl SettingsStore for LocalSettings {
    fn load(&self) -> SrsSettings {
        let mut s: SrsSettings = LocalStorage::get(SETTINGS_KEY).unwrap_or_default();
        if s.server_url.is_empty() {
            s.server_url = default_server_url();
        }
        s
    }

    fn load_async<'a>(&'a self) -> BoxFuture<'a, Result<SrsSettings, String>> {
        Box::pin(async move { Ok(self.load()) })
    }

    fn save<'a>(
        &'a self,
        s: SrsSettings,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            LocalStorage::set(SETTINGS_KEY, s).map_err(|e| e.to_string())
        })
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

    fn sync_now<'a>(&'a self) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            // Cancel any pending debounce — we're about to do the work now.
            if let Some(t) = self.state.borrow_mut().pending.take() {
                t.cancel();
            }
            if self.state.borrow().in_flight {
                self.state.borrow_mut().retry = true;
                return Ok(
                    "Sync already in progress — will repeat when it finishes.".into(),
                );
            }
            self.state.borrow_mut().in_flight = true;
            let result = do_sync(self.state.clone()).await;
            self.state.borrow_mut().in_flight = false;
            if std::mem::replace(&mut self.state.borrow_mut().retry, false) {
                self.schedule_sync();
            }
            result
        })
    }

    fn request_otp<'a>(
        &'a self,
        server_url: &'a str,
        email: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async move {
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
        })
    }

    fn verify_otp<'a>(
        &'a self,
        server_url: &'a str,
        email: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
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
        })
    }
}
