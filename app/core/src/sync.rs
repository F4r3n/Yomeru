//! Sync, without the UI: the wire format, the merge rules, and how a sync
//! reports its progress.
//!
//! The scheduler itself lives in [`crate::platform::SettingsStore`] —
//! web/android use the debouncer in `platform.rs`; the extension delegates to
//! `background.ts` via a `BUMP_DB_VERSION` / `SYNC_CARDS` message. Whatever
//! runs a sync reports it through [`SyncRunning`] and [`record_sync_result`],
//! which forward to the [`SyncObserver`] the UI installed.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::idb::Tombstone;
use crate::platform::Platform;
use crate::settings::SrsSettings;
use crate::types::{ReviewLog, SrsCard};

/// The status line after a manual sync. Counts what crossed the wire, not the
/// deck size: with delta sync a quiet sync moves nothing, and "Synced 0 cards"
/// would read as an empty deck.
pub fn sync_summary(sent: usize, received: usize) -> String {
    if sent == 0 && received == 0 {
        "Synced. Nothing changed.".to_string()
    } else {
        format!("Synced: {sent} sent, {received} received.")
    }
}

/// Error both sync clients (this crate and the extension's `sync.ts`) return
/// for a 401. The UI matches it to tell "sign in again" apart from a
/// transient failure, so the two strings must stay identical.
pub const SESSION_EXPIRED: &str = "session expired — re-verify";

/// Error returned when no server/token is configured. Not a failure worth
/// warning about: the user simply hasn't set sync up.
pub const NOT_AUTHENTICATED: &str = "not authenticated";

/// Where sync progress goes: the UI's spinner and "not synced" banner. Called
/// from wherever a sync runs, including timers outside any screen.
pub trait SyncObserver: Send + Sync {
    /// A sync started. Every call is matched by one [`Self::finished`].
    fn started(&self);
    fn finished(&self);
    /// The outcome of the sync that just ran.
    fn result(&self, result: Result<(), String>);
}

static OBSERVER: OnceLock<Box<dyn SyncObserver>> = OnceLock::new();

/// Installs the [`SyncObserver`]. Called once at startup; later calls are
/// ignored. Until then, progress isn't reported anywhere.
pub fn set_observer(observer: Box<dyn SyncObserver>) {
    let _ = OBSERVER.set(observer);
}

/// Report the outcome of a sync attempt, from wherever it ran. A no-op before
/// an observer is installed.
pub fn record_sync_result(result: Result<(), String>) {
    if let Some(o) = OBSERVER.get() {
        o.result(result);
    }
}

/// Marks a sync as running for as long as it lives. Held across the request,
/// so every way out of it — success, error, `?` — ends the activity.
pub struct SyncRunning(());

impl SyncRunning {
    pub fn start() -> Self {
        if let Some(o) = OBSERVER.get() {
            o.started();
        }
        Self(())
    }
}

impl Drop for SyncRunning {
    fn drop(&mut self) {
        if let Some(o) = OBSERVER.get() {
            o.finished();
        }
    }
}

/// Forces an immediate sync, bypassing the debounce. Used by the
/// "Sync now" button and the startup sync. Records the outcome with
/// [`record_sync_result`].
pub async fn sync_now(platform: &Platform) -> Result<String, String> {
    let settings = &platform.settings;
    // Signed out, the call returns at once: no point flashing the spinner.
    let _running = (!settings.load().server_token.is_empty()).then(SyncRunning::start);
    let result = settings.sync_now().await;
    record_sync_result(result.as_ref().map(|_| ()).map_err(Clone::clone));
    result
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

/// Most review-log rows one sync uploads. Rows are ~100 bytes on the wire,
/// so a batch stays far below the server's request body limit; a longer
/// outbox drains over consecutive syncs. Mirrors `OUTBOX_BATCH` in
/// `extension/src/background/sync.ts`.
pub(crate) const OUTBOX_BATCH: usize = 1_000;

/// Where this device's view of the server ends, as the server handed it out.
/// Opaque: stored and sent back as `since`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SyncCursor {
    pub epoch: String,
    /// An integer on the wire: the server's `i64`, which would reject the
    /// `2.0` an `f64` serializes as.
    pub rev: i64,
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
    /// The review outbox: answers not yet acknowledged by the server.
    pub(crate) reviews: &'a [ReviewLog],
    /// The cursor from the last successful sync; asks for a delta from it.
    /// Absent on a first sync, which uploads every card and gets every card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) since: Option<&'a SyncCursor>,
}

#[derive(Deserialize)]
pub(crate) struct SyncResponse {
    pub(crate) cards: Vec<SrsCard>,
    #[serde(default)]
    pub(crate) deletions: Vec<String>,
    #[serde(default)]
    pub(crate) settings: Option<SettingsPayload>,
    /// Drill answers per card id across the user's whole log. `None` from a
    /// server that predates the review log — which also means it dropped the
    /// uploaded rows, so the outbox must be kept for a later sync.
    #[serde(default)]
    pub(crate) drill_counts: Option<std::collections::HashMap<String, u32>>,
    /// Hand back as `since` next time. `None` from a server that predates
    /// delta sync, whose answers are always complete.
    #[serde(default)]
    pub(crate) cursor: Option<SyncCursor>,
    /// Whether `cards`, `deletions` and `drill_counts` are the complete state
    /// (`true`) or only what changed after `since` plus the server's copy of
    /// every id sent (`false`). Defaults to `true`: an old server sends no
    /// flag, and its answers are complete.
    #[serde(default = "complete")]
    pub(crate) full: bool,
}

fn complete() -> bool {
    true
}

/// Picks which server cards to write over the local copies: those whose merge
/// version is at least the local one. A card we hold no copy of always applies.
///
/// Mirrors the server's `upsert_cards` rule so both sides converge on the same
/// winner. Ties favour the incoming card, matching the server's `>=` — two
/// writes at the same millisecond are almost certainly the same write, and
/// disagreeing on the tiebreak is what would leave devices permanently apart.
///
/// One exception: a card this request `uploaded` whose local copy is still the
/// one uploaded takes the server's copy whatever its version. That copy is the
/// server's ruling on the upload, and can carry an older stamp than the local
/// one — the server clamps a fast device clock's stamps to its own time.
/// Keeping the local copy there would leave it clean yet "newer" than anything
/// the server sends, refusing other devices' edits until edited again here.
///
/// Lives here rather than in `platform.rs` because that module is wasm-only:
/// keeping the decision pure is what lets it be tested on the host target.
pub fn cards_to_apply(remote: &[SrsCard], local: &[SrsCard], uploaded: &[SrsCard]) -> Vec<SrsCard> {
    let local_versions: std::collections::HashMap<&str, f64> = local
        .iter()
        .map(|c| (c.id.as_str(), c.version_ms()))
        .collect();
    let uploaded_versions: std::collections::HashMap<&str, f64> = uploaded
        .iter()
        .map(|c| (c.id.as_str(), c.version_ms()))
        .collect();
    remote
        .iter()
        .filter(|c| {
            let Some(&mine) = local_versions.get(c.id.as_str()) else {
                return true;
            };
            #[allow(clippy::float_cmp)] // the same stored value, read back
            let untouched = uploaded_versions.get(c.id.as_str()) == Some(&mine);
            untouched || c.version_ms() >= mine
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
            again_count: 0,
            updated_ms,
        }
    }

    #[test]
    fn newer_remote_card_is_applied() {
        let remote = [card("a::recognition", 2_000.0)];
        let local = [card("a::recognition", 1_000.0)];
        let applied = cards_to_apply(&remote, &local, &[]);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].updated_ms, 2_000.0);
    }

    #[test]
    fn newer_local_card_is_kept() {
        // A review (or promotion, or reset) landed locally while the sync was
        // in flight — the server's older copy must not overwrite it.
        let remote = [card("a::recognition", 1_000.0)];
        let local = [card("a::recognition", 2_000.0)];
        assert!(cards_to_apply(&remote, &local, &[]).is_empty());
    }

    #[test]
    fn the_servers_ruling_on_an_untouched_upload_is_adopted() {
        // This device's clock runs fast: it uploaded at 9_000, the server
        // clamped that to its own 5_000 and echoed it back.
        let uploaded = [card("a::recognition", 9_000.0)];
        let remote = [card("a::recognition", 5_000.0)];
        assert_eq!(cards_to_apply(&remote, &uploaded, &uploaded).len(), 1);

        // Edited again while the sync was in flight: the local edit stands.
        let edited = [card("a::recognition", 9_500.0)];
        assert!(cards_to_apply(&remote, &edited, &uploaded).is_empty());
    }

    #[test]
    fn card_we_do_not_have_is_applied() {
        let remote = [card("a::recognition", 1_000.0)];
        let local: [SrsCard; 0] = [];
        assert_eq!(cards_to_apply(&remote, &local, &[]).len(), 1);
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

        assert!(cards_to_apply(&[stale.clone()], &[promoted.clone()], &[]).is_empty());
        let applied = cards_to_apply(&[promoted], &[stale], &[]);
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
        assert!(cards_to_apply(&[unreviewed.clone()], &[reviewed.clone()], &[]).is_empty());
        assert_eq!(cards_to_apply(&[reviewed], &[unreviewed], &[]).len(), 1);
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
        use crate::types::ReviewKind;
        use serde_json::Value;

        fn fixture(name: &str) -> Value {
            let path =
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/sync/").to_string() + name;
            let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
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
                again_count: 2,
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
                reviews: &[
                    ReviewLog {
                        card_id: "1467640::recognition".to_string(),
                        rating: 1,
                        kind: ReviewKind::Drill,
                        reviewed_ms: 1_756_950_000_000.0,
                    },
                    ReviewLog {
                        card_id: "1467640::recognition".to_string(),
                        rating: 3,
                        kind: ReviewKind::Review,
                        reviewed_ms: 1_756_960_000_000.0,
                    },
                ],
                since: None,
            };
            let sent: Value = serde_json::to_value(&body).expect("serialize body");
            assert_eq!(sent, fixture("request-app-client.json"));

            // Once the device holds a cursor, the same payload carries it.
            let cursor = SyncCursor {
                epoch: "3f9c0a7d51e2b864.1".to_string(),
                rev: 2,
            };
            let delta = SyncBody {
                since: Some(&cursor),
                ..body
            };
            let sent: Value = serde_json::to_value(&delta).expect("serialize delta body");
            assert_eq!(sent, fixture("request-delta-app-client.json"));
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
            let counts = resp.drill_counts.expect("drill counts");
            assert_eq!(counts.get("1467640::recognition"), Some(&1));
            assert!(resp.full);
            assert_eq!(
                resp.cursor,
                Some(SyncCursor {
                    epoch: "3f9c0a7d51e2b864.1".to_string(),
                    rev: 2,
                })
            );
        }

        #[test]
        fn the_summary_counts_traffic_not_the_deck() {
            assert_eq!(sync_summary(0, 0), "Synced. Nothing changed.");
            assert_eq!(sync_summary(1, 3), "Synced: 1 sent, 3 received.");
        }

        #[test]
        fn the_delta_response_fixture_parses() {
            let resp: SyncResponse =
                serde_json::from_value(fixture("response-delta.json")).expect("parse delta");
            assert!(
                !resp.full,
                "a delta's drill counts must be merged, not replace"
            );
            assert_eq!(resp.cursor.map(|c| c.rev), Some(3));
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
            assert!(resp.drill_counts.is_none());
            // No cursor and no flag: a pre-delta server, whose every answer
            // is the complete state.
            assert!(resp.cursor.is_none());
            assert!(resp.full);
        }
    }
}
