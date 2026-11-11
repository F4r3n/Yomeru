//! Sync from the UI's side: the reactive status the screens read (data
//! generation, health, activity) and shims over [`yomeru_core::sync`].
//!
//! These free functions keep existing call sites (every mutation site in
//! `routes/*`) source-stable.

use std::sync::Mutex;

use dioxus::prelude::*;

pub use yomeru_core::sync::*;

use crate::platform::Platform;

/// Reactive "data changed" generation, provided once at the app root by
/// [`crate::App`]. A successful sync bumps it; route load-effects read it
/// (via [`sync_generation`]) so they re-run and pick up freshly pulled
/// cards from IDB.
#[derive(Clone, Copy)]
pub struct SyncGen(pub Signal<u32>);

/// Subscribe the current reactive scope to the sync generation and return
/// its value. Called at the top of route load `use_effect`s so they re-run
/// when a sync completes.
pub fn sync_generation() -> u32 {
    *consume_context::<SyncGen>().0.read()
}

/// Run `reload` on mount and again every time a sync lands. Each data tab
/// passes its own loader — there's no single global store to refresh, since
/// pages keep their card state in local signals. Pages that must preserve
/// in-progress UI (e.g. an active review session) guard inside `reload`.
pub fn use_reload_on_sync(mut reload: impl FnMut() + 'static) {
    use_effect(move || {
        let _ = sync_generation();
        reload();
    });
}

/// Bump the sync generation, re-running any subscribed route load-effect.
/// Uses a non-subscribing read so a one-shot caller (e.g. the startup-sync
/// task) doesn't accidentally subscribe itself and loop.
pub fn bump_sync_generation() {
    let mut sync_gen = consume_context::<SyncGen>().0;
    let next = *sync_gen.peek() + 1;
    sync_gen.set(next);
}

/// Outcome of the most recent sync — manual, startup or background — provided
/// at the app root. Drives the "not synced" banner and the Settings badge.
/// Starts as `None` (nothing attempted yet) so the banner never flashes on load.
///
/// A [`SyncSignal`], so it can be written from whichever thread a sync
/// finishes on: on Android, Dioxus drives the app from more than one thread.
#[derive(Clone, Copy)]
pub struct SyncHealth(pub SyncSignal<Option<Result<(), String>>>);

/// The root's [`SyncHealth`], reachable from code that runs outside any
/// component: the auto-sync timer and the extension's storage listener.
/// Writing a signal from there is fine — Dioxus queues the re-render over a
/// channel rather than needing the runtime on the stack. A `static`, not a
/// `thread_local!`: the writer may be on another thread than the one that
/// mounted the root.
static HEALTH: Mutex<Option<SyncHealth>> = Mutex::new(None);

fn health() -> Option<SyncHealth> {
    *HEALTH.lock().ok()?
}

impl SyncHealth {
    /// Create the signal and register it for [`record_sync_result`]. Call once,
    /// from the app root's `use_context_provider`.
    pub fn new_root() -> Self {
        let health = Self(Signal::new_maybe_sync(None));
        if let Ok(mut h) = HEALTH.lock() {
            *h = Some(health);
        }
        health
    }

    /// The session was rejected: auto-sync is dead until the user signs in again.
    pub fn session_expired(&self) -> bool {
        matches!(&*self.0.read(), Some(Err(e)) if e == SESSION_EXPIRED)
    }

    /// The last sync failed for a reason worth surfacing (not "sync isn't set up").
    pub fn failure(&self) -> Option<String> {
        match &*self.0.read() {
            Some(Err(e)) if e != NOT_AUTHENTICATED => Some(e.clone()),
            _ => None,
        }
    }
}

/// How many syncs are running right now — startup, "Sync now" or the
/// debounced auto-sync — provided at the app root. Drives the topbar spinner,
/// so the user knows the numbers on screen may be about to change.
#[derive(Clone, Copy)]
pub struct SyncActivity(pub SyncSignal<u32>);

/// Same arrangement as [`HEALTH`]: the auto-sync timer runs outside any
/// component.
static ACTIVITY: Mutex<Option<SyncActivity>> = Mutex::new(None);

fn activity() -> Option<SyncActivity> {
    *ACTIVITY.lock().ok()?
}

impl SyncActivity {
    /// Create the counter and register it for [`SyncRunning`]. Call once, from
    /// the app root's `use_context_provider`.
    pub fn new_root() -> Self {
        let activity = Self(Signal::new_maybe_sync(0));
        if let Ok(mut a) = ACTIVITY.lock() {
            *a = Some(activity);
        }
        activity
    }

    pub fn active(&self) -> bool {
        *self.0.read() > 0
    }
}

/// Feeds core's sync progress into [`SyncHealth`] and [`SyncActivity`].
/// Installed by [`crate::launch_with`]. A no-op before the app root mounts.
pub(crate) struct SignalObserver;

impl SyncObserver for SignalObserver {
    fn started(&self) {
        if let Some(mut a) = activity() {
            *a.0.write() += 1;
        }
    }

    fn finished(&self) {
        if let Some(mut a) = activity() {
            let n = *a.0.peek();
            a.0.set(n.saturating_sub(1));
        }
    }

    fn result(&self, result: Result<(), String>) {
        match health() {
            Some(mut health) => health.0.set(Some(result)),
            None => warn!("[yomeru] sync result before the app root mounted: {result:?}"),
        }
    }
}

/// Forget the last sync outcome, e.g. after a fresh sign-in, so a stale
/// "session expired" warning doesn't linger until the next sync lands.
pub fn clear_sync_health() {
    if let Some(mut health) = health() {
        health.0.set(None);
    }
}

/// Arms a debounced auto-sync. No-op if the user hasn't configured a
/// server token. Safe to call after every IDB mutation — the debounce
/// coalesces bursts.
pub fn schedule_sync() {
    consume_context::<Platform>().settings.schedule_sync();
}

/// Forces an immediate sync, bypassing the debounce. Used by the
/// "Sync now" button in Settings and the startup sync. Records the outcome in
/// [`SyncHealth`].
pub async fn sync_now() -> Result<String, String> {
    yomeru_core::sync::sync_now(&consume_context::<Platform>()).await
}
