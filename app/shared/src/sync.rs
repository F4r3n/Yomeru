//! Auto-sync shims. The scheduler implementation lives in
//! [`crate::platform::SettingsStore`] — web/android use the
//! [`gloo_timers`] debouncer in `platform.rs`; the extension delegates to
//! `background.ts` via a `BUMP_DB_VERSION` / `SYNC_CARDS` message.
//!
//! These free functions keep existing call sites (every mutation site in
//! `routes/*`) source-stable.

use dioxus::prelude::*;

use crate::platform::Platform;
use crate::types::SrsCard;

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

/// Arms a debounced auto-sync. No-op if the user hasn't configured a
/// server token. Safe to call after every IDB mutation — the debounce
/// coalesces bursts.
pub fn schedule_sync() {
    consume_context::<Platform>().settings.schedule_sync();
}

/// Forces an immediate sync, bypassing the debounce. Used by the
/// "Sync now" button in Settings.
pub async fn sync_now() -> Result<String, String> {
    consume_context::<Platform>().settings.sync_now().await
}

/// Picks which server cards to write over the local copies: those whose merge
/// version is at least the local one. A card we hold no copy of always applies.
///
/// Mirrors the server's `upsert_cards` rule so both sides converge on the same
/// winner. Ties favour the incoming card, matching the server's `>=` — two
/// writes at the same millisecond are almost certainly the same write, and
/// disagreeing on the tiebreak is what would leave devices permanently apart.
///
/// Lives here rather than in `platform.rs` because that module is wasm-only:
/// keeping the decision pure is what lets it be tested on the host target.
pub fn cards_to_apply(remote: &[SrsCard], local: &[SrsCard]) -> Vec<SrsCard> {
    let local_versions: std::collections::HashMap<&str, f64> = local
        .iter()
        .map(|c| (c.id.as_str(), c.version_ms()))
        .collect();
    remote
        .iter()
        .filter(|c| {
            let local_version = local_versions.get(c.id.as_str()).copied().unwrap_or(0.0);
            c.version_ms() >= local_version
        })
        .cloned()
        .collect()
}

/// Picks which of the server's tombstones to replay against the local card
/// store.
///
/// A tombstone the server still lists is a delete that stands: had a re-add
/// superseded it, `upsert_cards` would have cleared it. So the only ids held
/// back are ones the server hasn't ruled on yet — those we sent tombstones for
/// in this very request, and those whose local card was added after we
/// uploaded. Deleting either would silently eat a re-add.
pub fn deletions_to_apply(
    remote: &[String],
    sent: &[String],
    local_cards: &[SrsCard],
    uploaded_at: f64,
) -> Vec<String> {
    let sent_ids: std::collections::HashSet<&str> = sent.iter().map(String::as_str).collect();
    let local_added: std::collections::HashMap<&str, f64> = local_cards
        .iter()
        .map(|c| (c.id.as_str(), c.added_ms))
        .collect();
    remote
        .iter()
        .filter(|id| !sent_ids.contains(id.as_str()))
        .filter(|id| {
            local_added
                .get(id.as_str())
                .is_none_or(|added| *added <= uploaded_at)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CardDirection, CardStatus};
    use srs_core::CardState;

    const UPLOADED_AT: f64 = 5_000.0;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn ids_we_sent_are_excluded_from_remote_deletions() {
        let resp = ids(&["1000001::recognition", "1000002::recognition"]);
        let sent = ids(&["1000001::recognition"]);
        assert_eq!(
            deletions_to_apply(&resp, &sent, &[], UPLOADED_AT),
            vec!["1000002::recognition".to_string()]
        );
    }

    #[test]
    fn empty_sent_means_all_remote_deletions_apply() {
        let resp = ids(&["1000001::recognition", "1000002::recognition"]);
        assert_eq!(deletions_to_apply(&resp, &[], &[], UPLOADED_AT).len(), 2);
    }

    #[test]
    fn empty_resp_means_nothing_to_apply() {
        let sent = ids(&["1000001::recognition"]);
        assert!(deletions_to_apply(&[], &sent, &[], UPLOADED_AT).is_empty());
    }

    #[test]
    fn card_added_after_upload_survives_the_tombstone() {
        // Re-added while the request was in flight, so it wasn't in the payload
        // the server ruled on. Deleting it here would silently eat the re-add;
        // the next sync uploads it and the server clears the tombstone.
        let resp = ids(&["a::recognition"]);
        let mut re_added = card("a::recognition", 6_000.0);
        re_added.added_ms = UPLOADED_AT + 1.0;
        assert!(deletions_to_apply(&resp, &[], &[re_added], UPLOADED_AT).is_empty());
    }

    #[test]
    fn card_the_server_already_saw_is_removed() {
        // Uploaded in this very request and the server still tombstoned it, so
        // the delete stands and the local copy goes.
        let resp = ids(&["a::recognition"]);
        let mut stale = card("a::recognition", 1_000.0);
        stale.added_ms = 1_000.0;
        assert_eq!(
            deletions_to_apply(&resp, &[], &[stale], UPLOADED_AT),
            vec!["a::recognition".to_string()]
        );
    }

    fn card(id: &str, updated_ms: f64) -> SrsCard {
        SrsCard {
            id: id.to_string(),
            sequence: 1_000_001,
            direction: CardDirection::Recognition,
            due_ms: 0.0,
            stability: 0.0,
            difficulty: 0.0,
            reps: 0,
            lapses: 0,
            state: CardState::New,
            last_review_ms: None,
            added_ms: 1_000.0,
            status: CardStatus::Staging,
            priority: 0,
            updated_ms,
        }
    }

    #[test]
    fn newer_remote_card_is_applied() {
        let remote = [card("a::recognition", 2_000.0)];
        let local = [card("a::recognition", 1_000.0)];
        let applied = cards_to_apply(&remote, &local);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].updated_ms, 2_000.0);
    }

    #[test]
    fn newer_local_card_is_kept() {
        // A review (or promotion, or reset) landed locally while the sync was
        // in flight — the server's older copy must not overwrite it.
        let remote = [card("a::recognition", 1_000.0)];
        let local = [card("a::recognition", 2_000.0)];
        assert!(cards_to_apply(&remote, &local).is_empty());
    }

    #[test]
    fn card_we_do_not_have_is_applied() {
        let remote = [card("a::recognition", 1_000.0)];
        let local: [SrsCard; 0] = [];
        assert_eq!(cards_to_apply(&remote, &local).len(), 1);
    }

    #[test]
    fn promotion_beats_a_stale_copy_with_a_later_review() {
        // The bug this whole change exists for: promoting Staging→Active never
        // touches `last_review_ms`, so under the old merge key a stale copy
        // carrying any review time reverted the promotion. Keyed on
        // `updated_ms`, the promotion wins on being the later write.
        let mut promoted = card("a::recognition", 5_000.0);
        promoted.status = CardStatus::Active;

        let mut stale = card("a::recognition", 3_000.0);
        stale.last_review_ms = Some(9_000.0);

        assert!(cards_to_apply(&[stale.clone()], &[promoted.clone()]).is_empty());
        let applied = cards_to_apply(&[promoted], &[stale]);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].status, CardStatus::Active);
    }

    #[test]
    fn legacy_zero_version_falls_back_to_review_and_added_times() {
        // Cards written before `updated_ms` existed deserialize with 0. The
        // fallback has to order them by something both devices already agree
        // on, or the first sync after upgrading is an arbitrary coin flip.
        let mut reviewed = card("a::recognition", 0.0);
        reviewed.last_review_ms = Some(8_000.0);
        let mut unreviewed = card("a::recognition", 0.0);
        unreviewed.last_review_ms = None;

        assert_eq!(reviewed.version_ms(), 8_000.0);
        assert_eq!(unreviewed.version_ms(), 1_000.0); // added_ms
        assert!(cards_to_apply(&[unreviewed.clone()], &[reviewed.clone()]).is_empty());
        assert_eq!(cards_to_apply(&[reviewed], &[unreviewed]).len(), 1);
    }
}
