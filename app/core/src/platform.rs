//! Platform abstraction: traits that swap between the web/android (HTTP-
//! backed) flavor of dict lookup + settings storage and an extension flavor
//! that messages a background script.
//!
//! Core functions that need the dictionary or the settings store take a
//! [`Platform`]. The Dioxus UI keeps one in context and calls them through
//! thin shims (`yomeru_shared::dict`, `yomeru_shared::settings`, …), so its
//! routes still see `dict::lookup(word)` etc.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use examples_types::ExampleEntry;
use jmdict_types::WordEntry;
use kanjidic_types::{KanjiEntry, PartInfo, PartSearch};
use serde::{Deserialize, Serialize};

use crate::rt;
use crate::settings::{SETTINGS_KEY, SrsSettings, default_server_url};
use crate::sync::{SettingsPayload, SyncBody, SyncResponse};
use crate::types::SrsCard;
use async_trait::async_trait;
use tracing::warn;
// Threads: the platform objects are `Send + Sync` on every target, so
// a `Platform` can be shared across threads (and handed to Dioxus' launch
// context). Their futures are `Send` natively, where a host may poll them on
// any thread; in the browser they can't be (they wrap JS promises), and
// there's only one thread anyway.

/// Read-only dictionary surface. Web/android use `HttpDict`, which posts
/// JSON to the yomeru-server; the extension implements this against
/// `browser.runtime.sendMessage` so its in-WASM dicts service the request.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait DictClient: Send + Sync {
    /// Start any expensive one-time setup in the background, so the first
    /// lookup doesn't pay for it. Called once at app start.
    fn warm_up(&self) {}
    async fn lookup(&self, word: &str) -> Result<Vec<WordEntry>, String>;
    async fn lookup_many(&self, words: &[String]) -> Result<Vec<Vec<WordEntry>>, String>;
    async fn lookup_by_sequence(&self, sequences: &[u32])
    -> Result<Vec<Option<WordEntry>>, String>;
    async fn lookup_prefix(&self, text: &str, max: u8) -> Result<Vec<WordEntry>, String>;
    async fn kanji_for(&self, word: &str) -> Result<Vec<KanjiEntry>, String>;
    async fn examples_for(&self, word: &str, max: u8) -> Result<Vec<ExampleEntry>, String>;
    /// Every part the kanji finder offers (see `kanjidic_core::parts`).
    async fn kanji_parts(&self) -> Result<Vec<PartInfo>, String>;
    /// The kanji holding all of `parts`, at most `max` of them.
    async fn kanji_with_parts(&self, parts: &str, max: u8) -> Result<PartSearch, String>;
}

/// Settings persistence + sync orchestration. `load()` is intentionally
/// synchronous so existing call sites (event handlers, derive-during-render
/// snippets) stay tight; implementations back it with a cached snapshot of
/// the actual store. Async [`Self::load_async`] is for one-shot hydration at
/// startup.
///

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait SettingsStore: Send + Sync {
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

    /// Whether sync may send the email and sign-in data off the device. Only
    /// the extension has a separate grant (Firefox's built-in data consent);
    /// elsewhere, signing in is the user's choice to sync.
    async fn sync_consent(&self) -> bool {
        true
    }

    /// Shows the platform's consent prompt for sync, returning whether it was
    /// granted. Firefox only shows it during a click: call it from a click
    /// handler's task with nothing awaited before it.
    async fn request_sync_consent(&self) -> bool {
        true
    }

    /// Whether this platform has a page-lookup on/off switch. Only the
    /// extension (which injects a content script) does; web/android have no
    /// page to enable lookups on, so the toggle UI is hidden there.
    fn supports_lookup_toggle(&self) -> bool {
        false
    }

    /// Read the global "page lookups enabled" flag. Defaults to `true` so a
    /// fresh install is active out of the box.
    async fn lookups_enabled(&self) -> Result<bool, String> {
        Ok(true)
    }

    /// Persist the global "page lookups enabled" flag. The extension content
    /// script watches `storage.local` for this key, so flipping it here takes
    /// effect on every open tab and every page loaded afterwards.
    async fn set_lookups_enabled(&self, _enabled: bool) -> Result<(), String> {
        Ok(())
    }
}

/// The host services a UI hands to core. Cheap to clone — both fields
/// are `Arc`-counted trait objects.
#[derive(Clone)]
pub struct Platform {
    pub dict: Arc<dyn DictClient>,
    pub settings: Arc<dyn SettingsStore>,
}

// ── Default (HTTP) implementation ─────────────────────────────────────

/// The web + android flavor: dict and settings via HTTP and [`rt`]'s
/// key-value store (localStorage in the browser), auto-sync via an
/// in-process debouncer.
pub fn default_http_platform() -> Platform {
    Platform {
        dict: Arc::new(HttpDict),
        settings: Arc::new(LocalSettings::new()),
    }
}

/// The default settings/sync store with a different dictionary — the
/// Android app's offline [`crate::local_dict::LocalDict`]. Sync still goes
/// to the configured server.
pub fn platform_with_dict(dict: Arc<dyn DictClient>) -> Platform {
    Platform {
        dict,
        settings: Arc::new(LocalSettings::new()),
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
#[derive(Deserialize)]
struct KanjiPartsResponse {
    parts: Vec<PartInfo>,
}
#[derive(Serialize)]
struct KanjiByPartsBody<'a> {
    parts: &'a str,
    max: u8,
}

async fn post_json<B: Serialize, R: for<'de> Deserialize<'de>>(
    path: &str,
    body: &B,
) -> Result<R, String> {
    let res = rt::post_json(&api_url(path), body, None).await?;
    if !res.ok() {
        return Err(format!("server {}", res.status));
    }
    res.json()
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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

    async fn kanji_parts(&self) -> Result<Vec<PartInfo>, String> {
        let parsed: KanjiPartsResponse = post_json("/api/kanji-parts", &()).await?;
        Ok(parsed.parts)
    }

    async fn kanji_with_parts(&self, parts: &str, max: u8) -> Result<PartSearch, String> {
        post_json("/api/kanji-by-parts", &KanjiByPartsBody { parts, max }).await
    }
}

// ---------- LocalSettings ----------

/// Settings backed by [`rt::kv_get`] / [`rt::kv_set`], sync scheduling by an
/// in-process debounce task.
struct LocalSettings {
    state: SharedSyncState,
}

/// Shared between the store and its debounce task, which may run on another
/// thread. Every read-then-write happens under one lock, so two syncs can't
/// both see "not in flight" and start; the lock is never held across an
/// `.await` (a held `MutexGuard` would make the future non-`Send`, which the
/// compiler rejects natively).
type SharedSyncState = Arc<Mutex<SyncState>>;

/// A panic elsewhere can't leave these three fields inconsistent with each
/// other, so a poisoned lock is still safe to use.
fn lock(state: &Mutex<SyncState>) -> MutexGuard<'_, SyncState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct SyncState {
    /// Bumped by every (re-)arm and by `sync_now`; a sleeping debounce task
    /// only syncs if the generation it was armed with is still current, so
    /// re-arming restarts the wait and `sync_now` cancels it.
    generation: u32,
    in_flight: bool,
    retry: bool,
}

const DEBOUNCE_MS: u32 = 2_000;

/// Runs a sync after the debounce. When it finishes with `retry` set — a
/// change landed mid-flight, or the review outbox still has a backlog — it
/// re-arms itself, so every pending change eventually goes up.
fn arm_sync(state: SharedSyncState) {
    let armed = {
        let mut st = lock(&state);
        st.generation = st.generation.wrapping_add(1);
        st.generation
    };
    rt::spawn_detached(async move {
        rt::sleep_ms(DEBOUNCE_MS).await;
        {
            let mut st = lock(&state);
            if st.generation != armed || st.in_flight {
                return;
            }
            st.in_flight = true;
        }
        let running = crate::sync::SyncRunning::start();
        let result = do_sync(&state).await;
        drop(running);
        if let Err(e) = &result {
            warn!("[yomeru] auto-sync failed: {e}");
        }
        crate::sync::record_sync_result(result.map(|_| ()));
        if finish_sync(&state) {
            arm_sync(state);
        }
    });
}

/// Clears `in_flight` and returns whether a change asked for another sync
/// while this one ran.
fn finish_sync(state: &Mutex<SyncState>) -> bool {
    let mut st = lock(state);
    st.in_flight = false;
    std::mem::replace(&mut st.retry, false)
}

impl LocalSettings {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SyncState::default())),
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

/// Step 1 of account deletion: the server emails a one-time code to the
/// account's address. `Ok(Some(code))` from a dev-mode server, which emails
/// nothing and returns the code instead.
///
/// Plain HTTP on every platform, the extension included: its pages can reach
/// any host, and nothing here needs the background's sync state — clearing
/// the token afterwards is what stops syncing, wherever sync runs.
pub async fn request_account_deletion(
    server_url: &str,
    token: &str,
) -> Result<Option<String>, String> {
    #[derive(Deserialize)]
    struct DevCode {
        dev_code: String,
    }
    let res = rt::post_json(
        &join_url(server_url, "/api/account/delete/request"),
        &serde_json::json!({}),
        Some(token),
    )
    .await?;
    match res.status {
        204 => Ok(None),
        200 => Ok(Some(res.json::<DevCode>()?.dev_code)),
        401 => Err("your session has expired. Sign in again first.".into()),
        429 => Err(
            "a code was sent less than a minute ago. Check your email, or wait and retry.".into(),
        ),
        s => Err(format!("server {s}")),
    }
}

/// Step 2: erases the account and all its data on the server, given the
/// emailed code.
pub async fn delete_account(server_url: &str, token: &str, code: &str) -> Result<(), String> {
    let res = rt::post_json(
        &join_url(server_url, "/api/account/delete"),
        &serde_json::json!({ "code": code }),
        Some(token),
    )
    .await?;
    match res.status {
        204 | 200 => Ok(()),
        401 => Err("your session has expired. Sign in again first.".into()),
        403 => Err("wrong or expired code.".into()),
        s => Err(format!("server {s}")),
    }
}

/// Who runs a sync server and how to reach them, as set by its operator at
/// deploy time. Either may be missing.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct PrivacyInfo {
    pub controller: Option<String>,
    pub contact: Option<String>,
}

pub async fn privacy_info(server_url: &str) -> Result<PrivacyInfo, String> {
    let res = rt::post_json(
        &join_url(server_url, "/api/privacy"),
        &serde_json::json!({}),
        None,
    )
    .await?;
    if !res.ok() {
        return Err(format!("server {}", res.status));
    }
    res.json()
}

fn join_url(base: &str, path: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}{path}")
}

async fn do_sync(state: &Mutex<SyncState>) -> Result<String, String> {
    use crate::idb::{
        apply_remote_deletions, clear_dirty, clear_tombstones, get_all_cards, get_all_tombstones,
        get_cards_by_ids, get_dirty, get_outbox, get_sync_cursor, set_sync_cursor, settle_outbox,
    };

    let s: SrsSettings = rt::kv_get(SETTINGS_KEY).unwrap_or_default();
    if s.server_url.is_empty() || s.server_token.is_empty() {
        return Err(crate::sync::NOT_AUTHENTICATED.into());
    }
    // Captured before the reads, not after: a card written between the read and
    // the timestamp would be missing from the payload yet still look like the
    // server had ruled on it, and a returned tombstone would delete it.
    let uploaded_at = rt::now_ms();
    let cursor = get_sync_cursor()
        .await
        .map_err(|e| format!("read sync cursor: {e}"))?;
    // Read before the cards, so a card changed after this snapshot either
    // misses the upload *and* stays dirty, or is in both.
    let dirty = get_dirty()
        .await
        .map_err(|e| format!("read dirty set: {e}"))?;
    // With a cursor, only what changed since goes up. Without one (first sync,
    // new account, wiped database) the whole deck does.
    let local_cards = if cursor.is_some() {
        let ids: Vec<String> = dirty.iter().map(|d| d.id.clone()).collect();
        get_cards_by_ids(&ids).await
    } else {
        get_all_cards().await
    }
    .map_err(|e| format!("read cards: {e}"))?;
    let local_tombstones = get_all_tombstones()
        .await
        .map_err(|e| format!("read tombstones: {e}"))?;
    let sent_ids: Vec<String> = local_tombstones.iter().map(|t| t.id.clone()).collect();
    let mut outbox = get_outbox()
        .await
        .map_err(|e| format!("read review outbox: {e}"))?;
    // A backlog (months of answers logged before signing in) goes up in
    // batches, so one request never outgrows the server's body limit.
    // Oldest first: the store's key order is by card id, not by time.
    outbox.sort_by(|a, b| a.reviewed_ms.total_cmp(&b.reviewed_ms));
    let backlog = outbox.len() > crate::sync::OUTBOX_BATCH;
    outbox.truncate(crate::sync::OUTBOX_BATCH);
    let mut settled = false;

    let res = rt::post_json(
        &join_url(s.server_url.trim(), "/api/sync"),
        &SyncBody {
            cards: &local_cards,
            deletions: &local_tombstones,
            settings: SettingsPayload::from_settings(&s),
            reviews: &outbox,
            since: cursor.as_ref(),
        },
        Some(&s.server_token),
    )
    .await?;
    if res.status == 401 {
        return Err(crate::sync::SESSION_EXPIRED.into());
    }
    if !res.ok() {
        return Err(format!("server {}", res.status));
    }
    let resp: SyncResponse = res.json()?;

    if !resp.cards.is_empty() {
        merge_remote_cards(&resp.cards, &local_cards).await?;
    }
    // Re-read the local cards: one added while the sync was in flight must be
    // visible here or the deletion filter can't tell it from a stale copy.
    let local_after_merge = crate::idb::get_all_cards()
        .await
        .map_err(|e| format!("read cards for deletions: {e}"))?;
    let foreign = crate::sync::deletions_to_apply(
        &resp.deletions,
        &sent_ids,
        &local_after_merge,
        uploaded_at,
    );
    apply_remote_deletions(&foreign)
        .await
        .map_err(|e| format!("apply deletions: {e}"))?;
    clear_tombstones(&sent_ids)
        .await
        .map_err(|e| format!("clear tombstones: {e}"))?;
    // The server has stored the outbox and counted it, so the cached counts
    // replace the sent rows rather than add to them. A server without the
    // review log sends no counts and kept nothing: hold the rows until it does.
    // A delta carries only the counts that changed, merged over the cache; a
    // full answer is the complete set and replaces it.
    if let Some(counts) = &resp.drill_counts {
        settle_outbox(&outbox, counts, resp.full)
            .await
            .map_err(|e| format!("settle review outbox: {e}"))?;
        settled = true;
    }
    clear_dirty(&dirty)
        .await
        .map_err(|e| format!("clear dirty set: {e}"))?;
    // Last: if anything above failed, the old cursor stays and the next sync
    // asks for these rows again, which merging makes harmless. A server that
    // predates delta sync sends none, and the next sync uploads everything.
    //
    // A full answer to a request that carried a cursor means the server
    // couldn't serve a delta from it: its database was restored or reset, or
    // the cursor belongs to another account. Either way it may lack cards that
    // only this device holds, and this request sent only the dirty ones. So
    // keep no cursor and sync again at once, uploading the whole deck.
    let refused = cursor.is_some() && resp.full;
    set_sync_cursor(if refused { None } else { resp.cursor.as_ref() })
        .await
        .map_err(|e| format!("store sync cursor: {e}"))?;
    if refused {
        lock(state).retry = true;
    }

    // Adopt server-side settings if they're newer. Re-read from localStorage
    // (not the start-of-sync snapshot) so a scheduler edit made while the sync
    // was in flight isn't clobbered, and preserve the device-local connection
    // fields the server never sees.
    if let Some(remote) = resp.settings {
        let mut merged: SrsSettings = rt::kv_get(SETTINGS_KEY).unwrap_or_default();
        if remote.updated_ms > merged.settings_updated_ms {
            merged.graduation_interval_days = remote.graduation_interval_days;
            merged.interval_scale = remote.interval_scale;
            merged.max_session_cards = remote.max_session_cards;
            merged.request_retention = remote.request_retention;
            merged.settings_updated_ms = remote.updated_ms;
            rt::kv_set(SETTINGS_KEY, &merged)?;
        }
    }

    // Drain a backlog on the next pass — only from a sync that fully
    // succeeded, so a failing one can't keep re-arming itself every 2 s.
    if settled && backlog {
        lock(state).retry = true;
    }
    Ok(crate::sync::sync_summary(
        local_cards.len(),
        resp.cards.len(),
    ))
}

/// Mirrors the server-side `updated_ms` last-write-wins rule on the client:
/// skip an incoming card if the local copy was written more recently (we
/// reviewed, promoted or reset it while the sync was in flight).
///
/// Writes through `put_cards_synced` so the adopted cards keep the version they
/// arrived with — stamping them here would mark every synced card as locally
/// modified and bounce it straight back at the server.
async fn merge_remote_cards(remote: &[SrsCard], uploaded: &[SrsCard]) -> Result<(), String> {
    use crate::idb::{get_all_cards, put_cards_synced};
    let local = get_all_cards()
        .await
        .map_err(|e| format!("read cards for merge: {e}"))?;
    let to_put = crate::sync::cards_to_apply(remote, &local, uploaded);
    put_cards_synced(&to_put)
        .await
        .map_err(|e| format!("put cards: {e}"))
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SettingsStore for LocalSettings {
    fn load(&self) -> SrsSettings {
        let mut s: SrsSettings = rt::kv_get(SETTINGS_KEY).unwrap_or_default();
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
        let prev: SrsSettings = rt::kv_get(SETTINGS_KEY).unwrap_or_default();
        let scheduler_changed = prev.graduation_interval_days != s.graduation_interval_days
            || prev.interval_scale != s.interval_scale
            || prev.max_session_cards != s.max_session_cards
            || prev.request_retention != s.request_retention;
        if scheduler_changed {
            s.settings_updated_ms = rt::now_ms();
        }
        rt::kv_set(SETTINGS_KEY, &s)
    }

    fn schedule_sync(&self) {
        // Don't even arm the timer if we have nothing configured.
        let s = self.load();
        if s.server_url.is_empty() || s.server_token.is_empty() {
            return;
        }
        {
            let mut st = lock(&self.state);
            if st.in_flight {
                st.retry = true;
                return;
            }
        }
        arm_sync(self.state.clone());
    }

    async fn sync_now(&self) -> Result<String, String> {
        // Cancel any pending debounce — we're about to do the work now.
        {
            let mut st = lock(&self.state);
            st.generation = st.generation.wrapping_add(1);
            if st.in_flight {
                st.retry = true;
                return Ok("Sync already in progress — will repeat when it finishes.".into());
            }
            st.in_flight = true;
        }
        let result = do_sync(&self.state).await;
        if finish_sync(&self.state) {
            self.schedule_sync();
        }
        result
    }

    async fn request_otp(&self, server_url: &str, email: &str) -> Result<Option<String>, String> {
        let res = rt::post_json(
            &join_url(server_url, "/api/auth/request"),
            &AuthRequestBody { email },
            None,
        )
        .await?;
        if !res.ok() {
            return Err(format!("server {}", res.status));
        }
        if res.status == 204 {
            return Ok(None);
        }
        let parsed: VerifyResponse = res.json()?;
        Ok(Some(parsed.token))
    }

    async fn verify_otp(
        &self,
        server_url: &str,
        email: &str,
        code: &str,
    ) -> Result<String, String> {
        let res = rt::post_json(
            &join_url(server_url, "/api/auth/verify"),
            &VerifyBody { email, code },
            None,
        )
        .await?;
        if !res.ok() {
            return Err(format!("server {}: {}", res.status, res.body));
        }
        let parsed: VerifyResponse = res.json()?;
        Ok(parsed.token)
    }
}
