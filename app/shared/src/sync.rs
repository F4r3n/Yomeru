//! Auto-sync shims. The scheduler implementation lives in
//! [`crate::platform::SettingsStore`] — web/android use the
//! [`gloo_timers`] debouncer in `platform.rs`; the extension delegates to
//! `background.ts` via a `BUMP_DB_VERSION` / `SYNC_CARDS` message.
//!
//! These free functions keep existing call sites (every mutation site in
//! `routes/*`) source-stable.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::idb::Tombstone;
use crate::platform::Platform;
use crate::settings::SrsSettings;
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

/// The exact JSON this client puts on the wire, and the JSON it parses back.
///
/// These live here rather than in `platform.rs` for the same reason the merge
/// rules do: that module is wasm-only, so the shapes would be untestable on the
/// host — and a wire format nothing pins is a wire format that drifts. The
/// fixture tests below compare them byte-for-byte against `testdata/sync/`,
/// which the server's own tests POST through the real router.
///
/// Synced scheduler settings, on the wire to/from the server. Field names are
/// snake_case to match the server's `db::Settings`. Local-only connection
/// fields (server_url/email/token) are deliberately absent.
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct SettingsPayload {
    pub(crate) graduation_interval_days: u32,
    pub(crate) interval_scale: f64,
    pub(crate) max_session_cards: u32,
    pub(crate) request_retention: f64,
    pub(crate) updated_ms: f64,
}

impl SettingsPayload {
    pub(crate) fn from_settings(s: &SrsSettings) -> Self {
        Self {
            graduation_interval_days: s.graduation_interval_days,
            interval_scale: s.interval_scale,
            max_session_cards: s.max_session_cards,
            request_retention: s.request_retention,
            updated_ms: s.settings_updated_ms,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct SyncBody<'a> {
    pub(crate) cards: &'a [SrsCard],
    /// Tombstones with the time the user actually deleted, so a delete made
    /// offline isn't stamped with upload time server-side. Serialized as
    /// objects; the server also still accepts the bare-id form older clients
    /// send, so it must be deployed before clients pick this up.
    pub(crate) deletions: &'a [Tombstone],
    pub(crate) settings: SettingsPayload,
}

#[derive(Deserialize, Default)]
pub(crate) struct SyncResponse {
    pub(crate) cards: Vec<SrsCard>,
    #[serde(default)]
    pub(crate) deletions: Vec<String>,
    #[serde(default)]
    pub(crate) settings: Option<SettingsPayload>,
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

    /// The bytes this client actually puts on the wire, pinned against the
    /// shared fixtures in `testdata/sync/` — the same files the server's HTTP
    /// tests POST through the real router. Between the two suites, a change to
    /// either side of the contract has to be made deliberately in both.
    ///
    /// The comparison is on `serde_json::Value`, which distinguishes
    /// `Number::from_f64(0.0)` from `Number::from(0u64)`. That is the whole
    /// point: `deleted_at` was once typed `i64` server-side, which would have
    /// rejected every request this client sends, because Rust writes an `f64`
    /// as `1757000000123.0`.
    mod wire {
        use super::*;
        use crate::idb::Tombstone;
        use serde_json::Value;

        fn fixture(name: &str) -> Value {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/sync/").to_string()
                + name;
            let raw = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {path}: {e}"));
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
        }

        fn fixture_card() -> SrsCard {
            SrsCard {
                id: "1467640::recognition".to_string(),
                sequence: 1_467_640,
                direction: CardDirection::Recognition,
                due_ms: 1_757_000_000_000.0,
                stability: 4.2,
                difficulty: 5.5,
                reps: 3,
                lapses: 1,
                state: CardState::Review,
                last_review_ms: Some(1_756_900_000_000.0),
                added_ms: 1_756_000_000_000.0,
                status: CardStatus::Active,
                priority: 0,
                updated_ms: 1_756_900_000_000.0,
            }
        }

        #[test]
        fn the_request_body_matches_the_fixture_exactly() {
            let cards = [fixture_card()];
            let deletions = [Tombstone {
                id: "1467640::recall".to_string(),
                deleted_at: 1_757_000_000_123.0,
            }];
            let body = SyncBody {
                cards: &cards,
                deletions: &deletions,
                settings: SettingsPayload {
                    graduation_interval_days: 365,
                    interval_scale: 1.0,
                    max_session_cards: 20,
                    request_retention: 0.9,
                    updated_ms: 1_756_000_000_000.0,
                },
            };
            let sent: Value = serde_json::to_value(&body).expect("serialize body");
            assert_eq!(sent, fixture("request-app-client.json"));
        }

        #[test]
        fn the_settings_payload_is_built_from_the_local_settings() {
            // The connection fields are local-only and must not reach the wire.
            let settings = SrsSettings {
                graduation_interval_days: 365,
                interval_scale: 1.0,
                max_session_cards: 20,
                request_retention: 0.9,
                server_url: "https://example.invalid".to_string(),
                server_email: "alice@example.com".to_string(),
                server_token: "secret".to_string(),
                settings_updated_ms: 1_756_000_000_000.0,
            };
            let sent = serde_json::to_value(SettingsPayload::from_settings(&settings))
                .expect("serialize settings");
            assert_eq!(sent, fixture("request-app-client.json")["settings"]);
        }

        #[test]
        fn the_response_fixture_parses_into_what_the_merge_consumes() {
            let resp: SyncResponse =
                serde_json::from_value(fixture("response.json")).expect("parse response");
            assert_eq!(resp.cards.len(), 1);
            let c = resp.cards.first().expect("one card");
            assert_eq!(c.id, "1467640::recognition");
            assert_eq!(c.sequence, 1_467_640);
            assert_eq!(c.reps, 3);
            assert_eq!(c.last_review_ms, Some(1_756_900_000_000.0));
            // The merge key, not the review time: a promotion carries no review.
            assert_eq!(c.version_ms(), 1_756_900_000_000.0);
            assert_eq!(resp.deletions, vec!["1467640::recall".to_string()]);
            let s = resp.settings.expect("settings");
            assert_eq!(s.graduation_interval_days, 365);
            assert_eq!(s.updated_ms, 1_756_000_000_000.0);
        }

        #[test]
        fn a_response_without_deletions_or_settings_still_parses() {
            // What an older server returns. Both fields are `#[serde(default)]`
            // precisely so a client update can ship before the server does.
            let resp: SyncResponse =
                serde_json::from_str(r#"{"cards":[]}"#).expect("parse minimal response");
            assert!(resp.cards.is_empty());
            assert!(resp.deletions.is_empty());
            assert!(resp.settings.is_none());
        }
    }
}
