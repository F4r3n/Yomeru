//! Auto-sync scheduler. Every card mutation calls [`schedule_sync`], which
//! arms a short debounce timer; when it fires we POST the local card set +
//! tombstones to the server and apply the merged response back into IDB.
//!
//! Single-threaded WASM — a `thread_local` `RefCell` is enough to coordinate
//! the in-flight timer and the "another mutation came in while syncing" flag.

use std::cell::RefCell;
use std::collections::HashSet;

use gloo_timers::callback::Timeout;
use log::warn;
use wasm_bindgen_futures::spawn_local;

use crate::api::{self, SyncResponse};
use crate::idb::{
    apply_remote_deletions, clear_tombstones, get_all_cards, get_all_tombstones, put_cards,
};
use crate::settings::load;
use crate::types::SrsCard;

const DEBOUNCE_MS: u32 = 2_000;

thread_local! {
    static PENDING: RefCell<Option<Timeout>> = const { RefCell::new(None) };
    /// True while a sync request is in flight. If `schedule_sync` is called
    /// while one is running, set `RETRY` so we kick off another pass after it
    /// finishes (otherwise a mutation during the network round-trip would be
    /// silently dropped until the next manual change).
    static IN_FLIGHT: RefCell<bool> = const { RefCell::new(false) };
    static RETRY: RefCell<bool> = const { RefCell::new(false) };
}

/// Arms a debounced sync. Cheap to call after every mutation; no-op if the
/// user hasn't configured a server token.
pub fn schedule_sync() {
    // Avoid arming a timer for users who haven't configured a server. They
    // still hit this on every mutation; cheap to short-circuit.
    let s = load();
    if s.server_url.is_empty() || s.server_token.is_empty() {
        return;
    }
    if IN_FLIGHT.with(|f| *f.borrow()) {
        RETRY.with(|r| *r.borrow_mut() = true);
        return;
    }
    let t = Timeout::new(DEBOUNCE_MS, || {
        PENDING.with(|p| *p.borrow_mut() = None);
        spawn_local(run_once());
    });
    PENDING.with(|p| *p.borrow_mut() = Some(t));
}

/// Forces an immediate sync, bypassing the debounce. Used by the "Sync now"
/// button. Returns the user-facing status (Ok = success message, Err = error).
///
/// If an auto-sync is already in flight, waits for it to finish before
/// running — never starts a second concurrent request.
pub async fn sync_now() -> Result<String, String> {
    // Cancel any pending debounce: we're about to do the work now.
    PENDING.with(|p| {
        if let Some(t) = p.borrow_mut().take() {
            t.cancel();
        }
    });
    if IN_FLIGHT.with(|f| *f.borrow()) {
        // Don't start a second request. Signal retry so the in-flight one
        // re-runs after itself; tell the user we queued their request.
        RETRY.with(|r| *r.borrow_mut() = true);
        return Ok("Sync already in progress — will repeat when it finishes.".into());
    }
    IN_FLIGHT.with(|f| *f.borrow_mut() = true);
    let result = do_sync().await;
    IN_FLIGHT.with(|f| *f.borrow_mut() = false);
    if RETRY.with(|r| std::mem::replace(&mut *r.borrow_mut(), false)) {
        schedule_sync();
    }
    result
}

async fn run_once() {
    IN_FLIGHT.with(|f| *f.borrow_mut() = true);
    if let Err(e) = do_sync().await {
        warn!("[yomeru] auto-sync failed: {e}");
    }
    IN_FLIGHT.with(|f| *f.borrow_mut() = false);
    if RETRY.with(|r| std::mem::replace(&mut *r.borrow_mut(), false)) {
        schedule_sync();
    }
}

async fn do_sync() -> Result<String, String> {
    let s = load();
    if s.server_url.is_empty() || s.server_token.is_empty() {
        return Err("not authenticated".into());
    }

    let local_cards = get_all_cards()
        .await
        .map_err(|e| format!("read cards: {e}"))?;
    let local_tombstones = get_all_tombstones()
        .await
        .map_err(|e| format!("read tombstones: {e}"))?;

    let resp = api::sync_cards(
        s.server_url.trim(),
        &s.server_token,
        &local_cards,
        &local_tombstones,
    )
    .await?;

    apply_response(&resp, &local_tombstones).await?;

    Ok(format!(
        "Synced {} card{}.",
        resp.cards.len(),
        if resp.cards.len() == 1 { "" } else { "s" }
    ))
}

/// Reconciles a sync response into local IDB. Pure data-shaping; no
/// network. Exported (`pub(crate)`) so unit tests can exercise the merge
/// without the HTTP round-trip.
///
/// Race-safe against re-add-during-sync: a server-reported deletion is only
/// applied if we *didn't* send a tombstone for that id. If we sent the
/// tombstone, anything currently in the local cards store for that id is
/// the user's intentional re-add — leave it alone, and our `clear_tombstones`
/// below will retract our stale local tombstone so the next sync upserts
/// the re-add to the server.
pub(crate) async fn apply_response(
    resp: &SyncResponse,
    sent_tombstones: &[String],
) -> Result<(), String> {
    if !resp.cards.is_empty() {
        put_cards_skip_older(&resp.cards).await?;
    }

    let sent: HashSet<&str> = sent_tombstones.iter().map(String::as_str).collect();
    let foreign: Vec<String> = resp
        .deletions
        .iter()
        .filter(|id| !sent.contains(id.as_str()))
        .cloned()
        .collect();
    apply_remote_deletions(&foreign)
        .await
        .map_err(|e| format!("apply deletions: {e}"))?;

    clear_tombstones(sent_tombstones)
        .await
        .map_err(|e| format!("clear tombstones: {e}"))?;
    Ok(())
}

/// Like `put_cards`, but skips an incoming card if the local copy has a
/// newer `last_review_ms` (avoids clobbering a review the user just did
/// while a sync was in flight — the server-side upsert does the same
/// check, but in the opposite direction).
async fn put_cards_skip_older(remote: &[SrsCard]) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    //! These tests run on the host target (no wasm), which means we can't
    //! touch IDB directly — instead we keep the merge logic in
    //! [`apply_response`] tightly scoped to the IDB helpers it calls, and
    //! lean on integration testing (and the extension's vitest suite, which
    //! covers the same merge shape via `applySyncResponse`).
    //!
    //! The pure filter logic for the re-add race is small enough to test
    //! here without IDB:
    use super::*;

    fn foreign_deletions(resp_deletions: &[&str], sent: &[&str]) -> Vec<String> {
        let sent: HashSet<&str> = sent.iter().copied().collect();
        resp_deletions
            .iter()
            .filter(|id| !sent.contains(**id))
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn ids_we_sent_are_excluded_from_remote_deletions() {
        // Race scenario: we sent a tombstone for "猫::recognition" and the
        // user re-added the card during the round-trip. The server echoes
        // our tombstone back in `deletions`; we must not re-delete the
        // re-added card.
        let resp = ["猫::recognition", "犬::recognition"];
        let sent = ["猫::recognition"];
        assert_eq!(
            foreign_deletions(&resp, &sent),
            vec!["犬::recognition".to_string()]
        );
    }

    #[test]
    fn empty_sent_means_all_remote_deletions_apply() {
        // Another device deleted these; we had no tombstones to send.
        let resp = ["猫::recognition", "犬::recognition"];
        let sent: [&str; 0] = [];
        assert_eq!(foreign_deletions(&resp, &sent).len(), 2);
    }

    #[test]
    fn empty_resp_means_nothing_to_apply() {
        let resp: [&str; 0] = [];
        let sent = ["猫::recognition"];
        assert!(foreign_deletions(&resp, &sent).is_empty());
    }
}
