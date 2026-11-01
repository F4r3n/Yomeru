//! HTTP-level tests for `/api/sync`, driving the real router with real JSON.
//!
//! The `db` modules already test the merge rules by calling them directly. What
//! they can't cover is the layer where the two clients actually meet the server:
//! serde on a request body, the handler's transaction, and the response shape.
//! That layer is where the one bug that escaped review lived — `deleted_at` was
//! declared `i64` while both clients send a JS `Date.now()`, which serde emits
//! as `1757000000123.0`, so every sync would have 400'd on a body no unit test
//! ever constructed.
//!
//! These tests replace the two-device manual check: each scenario runs the round
//! trip twice and asserts the second round changes nothing. The fixpoint is the
//! part that matters — divergence shows up as a device that reverts its peer on
//! the *next* sync, not as a wrong answer on the first.
//!
//! [`Device`] adopts whatever the server returns; it deliberately does not
//! reimplement the client-side merge. The claims here are about what the server
//! stores and hands back. The client's own merge is covered by the unit tests in
//! `app/shared/src/sync.rs` and `extension/src/background/idb.test.ts`, and the
//! payloads the clients really produce are pinned by the fixtures in `wire`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use governor::{Quota, RateLimiter};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::AppState;
use crate::client_ip::TrustProxy;
use crate::config::Config;
use crate::db;

const EMAIL: &str = "alice@example.com";

/// A fixed instant in the past that every fixture time is an offset from.
/// Deliberately behind real wall-clock: the server clamps both card versions
/// and delete times to its own `now`, so future-dated fixtures would silently
/// collapse onto each other and the ordering under test would vanish.
const T0: f64 = 1_700_000_000_000.0;

const SECOND: f64 = 1_000.0;
const DAY: f64 = 86_400_000.0;

// ---- harness ------------------------------------------------------------

struct Harness {
    app: Router,
    db: db::Db,
    token: String,
}

impl Harness {
    async fn new() -> Self {
        // Production quota is 10/min on the auth routes and every simulated
        // device shares 127.0.0.1, so a multi-round convergence test would trip
        // the limits. Rate limiting has its own coverage; here it would only
        // add flake.
        let quota = Quota::per_second(const { NonZeroU32::new(10_000).unwrap() });
        Self::with_quotas(quota, quota).await
    }

    async fn with_quotas(auth: Quota, sync: Quota) -> Self {
        let db = db::test_support::single_conn_mem().await;
        let token = "test-session-token".to_string();
        // Seeded directly rather than through the OTP flow: these tests are
        // about sync, and /api/auth/request would need SMTP or dev mode.
        db::create_session(&db, &token, EMAIL, i64::MAX)
            .await
            .unwrap();

        let cfg = Config {
            port: 0,
            bind: "127.0.0.1".to_string(),
            db_path: ":memory:".to_string(),
            data_dir: String::new(),
            trust_proxy: TrustProxy::Private,
            proxy_hops: 1,
            smtp_host: String::new(),
            smtp_port: 0,
            smtp_from: String::new(),
            smtp_user: None,
            smtp_pass: None,
            // Off, so the Bearer check is genuinely exercised.
            dev_mode: false,
        };

        let state = AppState {
            db: db.clone(),
            cfg: Arc::new(cfg),
            limiter: Arc::new(RateLimiter::keyed(auth)),
            sync_limiter: Arc::new(RateLimiter::keyed(sync)),
            lookup_limiter: Arc::new(RateLimiter::keyed(sync)),
        };

        Self {
            app: crate::api::router(state),
            db,
            token,
        }
    }

    async fn post(&self, path: &str, token: Option<&str>, body: &Value) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap();
        if let Some(t) = token {
            req.headers_mut().insert(
                header::AUTHORIZATION,
                format!("Bearer {t}").parse().unwrap(),
            );
        }
        // `oneshot` bypasses the connect-info layer that `axum::serve` installs,
        // so the extractor the handler uses for its rate-limit key has to be
        // populated by hand.
        let peer: SocketAddr = "127.0.0.1:51000".parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(peer));

        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    /// Card ids the server currently holds for the test user, sorted.
    async fn stored_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = db::get_all_cards(&self.db, EMAIL)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        ids.sort();
        ids
    }

    async fn tombstone_ids(&self) -> Vec<String> {
        let mut ids = db::get_all_deletions(&self.db, EMAIL).await.unwrap();
        ids.sort();
        ids
    }
}

#[derive(Deserialize)]
struct SyncResp {
    cards: Vec<db::Card>,
    #[serde(default)]
    deletions: Vec<String>,
    #[serde(default)]
    drill_counts: HashMap<String, u32>,
}

impl SyncResp {
    fn card(&self, id: &str) -> Option<&db::Card> {
        self.cards.iter().find(|c| c.id == id)
    }
}

// ---- a simulated device -------------------------------------------------

/// One client's local state, as a client without a cursor behaves: uploads its
/// whole deck plus pending tombstones — the reason the tombstone guards exist
/// at all — then adopts the server's answer. Delta clients are [`delta`]'s.
struct Device {
    cards: HashMap<String, db::Card>,
    /// `(id, deleted_at)` in client wall-clock ms. Held as `f64` so the request
    /// carries the float form the Rust client emits, which is the shape that
    /// previously failed to deserialize.
    tombstones: Vec<(String, f64)>,
}

impl Device {
    fn new(cards: &[db::Card]) -> Self {
        Self {
            cards: cards.iter().map(|c| (c.id.clone(), c.clone())).collect(),
            tombstones: Vec::new(),
        }
    }

    fn edit(&mut self, id: &str, f: impl FnOnce(&mut db::Card)) {
        if let Some(c) = self.cards.get_mut(id) {
            f(c);
        }
    }

    fn delete(&mut self, id: &str, at: f64) {
        self.cards.remove(id);
        self.tombstones.push((id.to_string(), at));
    }

    fn body(&self) -> Value {
        let mut cards: Vec<&db::Card> = self.cards.values().collect();
        cards.sort_by(|a, b| a.id.cmp(&b.id));
        json!({
            "cards": cards,
            "deletions": self.tombstones
                .iter()
                .map(|(id, at)| json!({ "id": id, "deleted_at": at }))
                .collect::<Vec<_>>(),
        })
    }

    async fn sync(&mut self, h: &Harness) -> SyncResp {
        let (status, body) = h.post("/api/sync", Some(&h.token), &self.body()).await;
        assert_eq!(status, StatusCode::OK, "sync rejected: {body}");
        let resp: SyncResp = serde_json::from_value(body).unwrap();
        // Server-authoritative adoption: replace the local set with the merged
        // one and drop the tombstones the server has now acknowledged.
        self.cards = resp
            .cards
            .iter()
            .map(|c| (c.id.clone(), c.clone()))
            .collect();
        self.tombstones.clear();
        resp
    }

    fn has(&self, id: &str) -> bool {
        self.cards.contains_key(id)
    }

    fn get(&self, id: &str) -> &db::Card {
        self.cards.get(id).unwrap_or_else(|| panic!("no card {id}"))
    }
}

/// A card as a client would hold it: `added_ms` and the merge version are set
/// explicitly, because every guard under test compares one against the other.
fn card(id: &str, added_ms: f64, updated_ms: f64) -> db::Card {
    db::Card {
        id: id.to_string(),
        sequence: 1_467_640,
        direction: "recognition".to_string(),
        due_ms: 0.0,
        stability: 0.0,
        difficulty: 0.0,
        reps: 0,
        lapses: 0,
        state: "new".to_string(),
        last_review_ms: None,
        added_ms,
        status: "active".to_string(),
        priority: 0,
        again_count: 0,
        updated_ms,
    }
}

const X: &str = "1467640::recognition";

// ---- scenarios ----------------------------------------------------------

#[tokio::test]
async fn a_delete_on_one_device_reaches_the_other_and_stays_gone() {
    let h = Harness::new().await;
    let mut a = Device::new(&[card(X, T0, T0)]);
    let mut b = Device::new(&[]);

    a.sync(&h).await;
    b.sync(&h).await;
    assert!(b.has(X), "B should have picked up the card first");

    a.delete(X, T0 + SECOND);
    let resp = a.sync(&h).await;
    assert!(resp.card(X).is_none());
    assert_eq!(resp.deletions, vec![X.to_string()]);

    // B still holds its copy and re-uploads it, as every client does.
    let resp = b.sync(&h).await;
    assert!(
        resp.card(X).is_none(),
        "B's stale copy resurrected the card"
    );
    assert!(!b.has(X));

    // The fixpoint: another full round must not bring it back. This is what
    // catches a device that reverts its peer one sync later.
    a.sync(&h).await;
    b.sync(&h).await;
    assert!(!a.has(X) && !b.has(X));
    assert!(h.stored_ids().await.is_empty());
    assert_eq!(h.tombstone_ids().await, vec![X.to_string()]);
}

#[tokio::test]
async fn a_promotion_survives_a_stale_copy_carrying_a_later_review() {
    // The bug this branch exists for. Promoting Staging→Active never touches
    // `last_review_ms`, so under the old merge key any stale copy with a review
    // time reverted the promotion — and it reverted again on every later sync.
    let h = Harness::new().await;
    let mut a = Device::new(&[{
        let mut c = card(X, T0, T0);
        c.status = "staging".to_string();
        c
    }]);
    a.sync(&h).await;

    let mut b = Device::new(&[]);
    b.sync(&h).await;

    a.edit(X, |c| {
        c.status = "active".to_string();
        c.updated_ms = T0 + 5.0 * SECOND;
    });
    a.sync(&h).await;

    // B's copy is older by the merge key but newer by the review time.
    b.edit(X, |c| c.last_review_ms = Some(T0 + 9.0 * SECOND));
    let resp = b.sync(&h).await;
    assert_eq!(resp.card(X).unwrap().status, "active");

    a.sync(&h).await;
    b.sync(&h).await;
    assert_eq!(a.get(X).status, "active");
    assert_eq!(b.get(X).status, "active");
}

#[tokio::test]
async fn a_reset_survives_a_stale_copy_that_still_has_the_reps() {
    // `reset_progression` clears reps and sets `last_review_ms` back to NULL,
    // moving the old merge key *backwards*. Keyed on `updated_ms` the reset is
    // simply the later write.
    let h = Harness::new().await;
    let mut a = Device::new(&[{
        let mut c = card(X, T0, T0 + 3.0 * SECOND);
        c.reps = 5;
        c.last_review_ms = Some(T0 + 3.0 * SECOND);
        c
    }]);
    a.sync(&h).await;
    let mut b = Device::new(&[]);
    b.sync(&h).await;
    assert_eq!(b.get(X).reps, 5);

    a.edit(X, |c| {
        c.reps = 0;
        c.last_review_ms = None;
        c.updated_ms = T0 + 6.0 * SECOND;
    });
    a.sync(&h).await;

    let resp = b.sync(&h).await;
    assert_eq!(resp.card(X).unwrap().reps, 0);
    assert_eq!(resp.card(X).unwrap().last_review_ms, None);

    a.sync(&h).await;
    b.sync(&h).await;
    assert_eq!(a.get(X).reps, 0);
    assert_eq!(b.get(X).reps, 0);
}

#[tokio::test]
async fn a_kanji_card_reaches_the_other_device_unchanged() {
    // Kanji cards reuse the card shape with a new `direction` and the kanji's
    // code point as `sequence`. The server relays both without interpreting
    // them, so no schema change is needed — this pins that down.
    const NEKO: &str = "29483::kanji";
    let h = Harness::new().await;
    let mut kanji = card(NEKO, T0, T0);
    kanji.sequence = 0x732b;
    kanji.direction = "kanji".to_string();
    kanji.reps = 3;
    let mut a = Device::new(&[kanji]);
    a.sync(&h).await;

    let mut b = Device::new(&[]);
    b.sync(&h).await;
    let got = b.get(NEKO);
    assert_eq!(got.sequence, 0x732b);
    assert_eq!(got.direction, "kanji");
    assert_eq!(got.reps, 3);
}

#[tokio::test]
async fn concurrent_offline_reviews_converge_on_the_later_one() {
    let h = Harness::new().await;
    let mut a = Device::new(&[card(X, T0, T0)]);
    a.sync(&h).await;
    let mut b = Device::new(&[]);
    b.sync(&h).await;

    // Both offline. A reviews first, B second.
    a.edit(X, |c| {
        c.reps = 1;
        c.stability = 1.5;
        c.last_review_ms = Some(T0 + SECOND);
        c.updated_ms = T0 + SECOND;
    });
    b.edit(X, |c| {
        c.reps = 1;
        c.stability = 4.2;
        c.last_review_ms = Some(T0 + 2.0 * SECOND);
        c.updated_ms = T0 + 2.0 * SECOND;
    });

    a.sync(&h).await;
    let resp = b.sync(&h).await;
    assert_eq!(resp.card(X).unwrap().stability, 4.2);

    // A pulls B's review rather than pushing its own back over it.
    a.sync(&h).await;
    assert_eq!(a.get(X).stability, 4.2);

    a.sync(&h).await;
    b.sync(&h).await;
    assert_eq!(a.get(X).stability, 4.2);
    assert_eq!(b.get(X).stability, 4.2);
}

#[tokio::test]
async fn a_stale_replica_cannot_resurrect_a_deleted_card() {
    let h = Harness::new().await;
    let mut a = Device::new(&[card(X, T0, T0)]);
    a.sync(&h).await;
    let mut b = Device::new(&[]);
    b.sync(&h).await;

    a.delete(X, T0 + SECOND);
    a.sync(&h).await;

    // B never pulled the delete and keeps re-uploading its copy, whose
    // `added_ms` predates the tombstone.
    let resp = b.sync(&h).await;
    assert!(resp.card(X).is_none());
    assert_eq!(
        resp.deletions,
        vec![X.to_string()],
        "the tombstone must survive the stale upload, or no device ever learns of the delete"
    );
    assert_eq!(h.tombstone_ids().await, vec![X.to_string()]);
}

#[tokio::test]
async fn a_genuine_re_add_beats_the_tombstone_and_clears_it() {
    let h = Harness::new().await;
    let mut a = Device::new(&[card(X, T0, T0)]);
    a.sync(&h).await;

    a.delete(X, T0 + SECOND);
    a.sync(&h).await;
    assert_eq!(h.tombstone_ids().await, vec![X.to_string()]);

    // The user adds the word again, after the delete.
    let readd = card(X, T0 + 2.0 * SECOND, T0 + 2.0 * SECOND);
    let mut b = Device::new(&[readd]);
    let resp = b.sync(&h).await;
    assert!(resp.card(X).is_some(), "a real re-add must be accepted");
    assert!(resp.deletions.is_empty(), "the tombstone should be cleared");

    let resp = a.sync(&h).await;
    assert!(
        resp.card(X).is_some(),
        "A should get the re-added card back"
    );
}

#[tokio::test]
async fn an_offline_delete_is_ordered_by_when_it_happened_not_when_it_arrived() {
    // A deletes while offline, then stays offline for a day. B re-adds the word
    // in the meantime and syncs first. When A finally uploads its pending
    // tombstone, the re-add must win — it happened later. If the server stamped
    // the tombstone with its own receipt time (which is now, long after both),
    // the delete would outrank the re-add and quietly destroy it.
    let h = Harness::new().await;
    let deleted_at = T0;
    let re_added_at = T0 + DAY;

    let mut b = Device::new(&[card(X, re_added_at, re_added_at)]);
    b.sync(&h).await;

    let mut a = Device::new(&[]);
    a.tombstones.push((X.to_string(), deleted_at));
    let resp = a.sync(&h).await;

    assert!(
        resp.card(X).is_some(),
        "the re-add postdates the delete and must survive it"
    );
    assert!(
        h.tombstone_ids().await.is_empty(),
        "a delete that lost must not leave a tombstone behind to re-kill the card later"
    );

    b.sync(&h).await;
    assert!(b.has(X));
}

#[tokio::test]
async fn a_future_dated_delete_cannot_outrank_every_later_re_add() {
    // A device with a badly fast clock. `deleted_at` is clamped to server time,
    // so the delete lands at "now" instead of years ahead, and an add made
    // after it still wins.
    let h = Harness::new().await;
    let mut a = Device::new(&[]);
    a.tombstones.push((X.to_string(), T0 + 3_650.0 * DAY)); // ~10 years out
    a.sync(&h).await;

    let stored = db::test_support::deleted_at(&h.db, EMAIL, X)
        .await
        .expect("tombstone recorded");
    #[allow(clippy::cast_precision_loss)]
    let stored_f = stored as f64;
    assert!(
        stored_f < T0 + 3_650.0 * DAY,
        "a client clock must not be able to claim a delete in the far future"
    );
}

// ---- auth ---------------------------------------------------------------

#[tokio::test]
async fn sync_without_a_token_is_rejected_and_writes_nothing() {
    let h = Harness::new().await;
    let mut a = Device::new(&[card(X, T0, T0)]);
    a.sync(&h).await;
    assert_eq!(h.stored_ids().await, vec![X.to_string()]);

    // An unauthenticated request carrying a delete must not touch anything.
    let body = json!({
        "cards": [],
        "deletions": [{ "id": X, "deleted_at": T0 + SECOND }],
    });
    let (status, _) = h.post("/api/sync", None, &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(h.stored_ids().await, vec![X.to_string()]);
    assert!(h.tombstone_ids().await.is_empty());
}

#[tokio::test]
async fn sync_with_an_expired_session_is_rejected() {
    let h = Harness::new().await;
    db::create_session(&h.db, "stale-token", EMAIL, 1)
        .await
        .unwrap();
    let (status, _) = h
        .post("/api/sync", Some("stale-token"), &json!({ "cards": [] }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---- wire fixtures ------------------------------------------------------

/// The request bodies the two real clients emit, and the response they parse.
///
/// The fixtures are shared with `app/shared/src/sync.rs` and
/// `extension/src/background/sync.test.ts`, which assert that each client
/// actually produces its file. Here we assert the other half: that the server
/// accepts each one over HTTP and stores the right thing.
///
/// The dialects differ on purpose. Rust serializes an `f64` timestamp as
/// `1757000000123.0`; JavaScript has one number type and `JSON.stringify` emits
/// the same value as `1757000000123`. The server has to take both, and getting
/// that wrong is exactly the bug these fixtures exist to prevent.
mod wire {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../testdata/sync/").to_string() + name;
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse fixture {path}: {e}"))
    }

    #[tokio::test]
    async fn the_rust_client_body_round_trips() {
        let h = Harness::new().await;
        let (status, body) = h
            .post(
                "/api/sync",
                Some(&h.token),
                &fixture("request-app-client.json"),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "server rejected the app client: {body}"
        );

        let resp: SyncResp = serde_json::from_value(body).unwrap();
        assert_eq!(resp.cards.len(), 1);
        assert_eq!(resp.deletions, vec!["1467640::recall".to_string()]);
        assert_eq!(resp.cards[0].again_count, 2, "again_count must round-trip");
        // One drill row and one review row: only the drill counts.
        assert_eq!(resp.drill_counts.get("1467640::recognition"), Some(&1));
        // Float `deleted_at` must survive as a real time, not collapse to
        // receipt time — that is the whole point of sending it.
        let stored = db::test_support::deleted_at(&h.db, EMAIL, "1467640::recall")
            .await
            .expect("tombstone recorded");
        assert_eq!(stored, 1_757_000_000_123);
    }

    #[tokio::test]
    async fn the_extension_body_round_trips() {
        // Same payload in JavaScript's dialect: integer-valued floats, including
        // inside the `settings` block.
        let h = Harness::new().await;
        let (status, body) = h
            .post(
                "/api/sync",
                Some(&h.token),
                &fixture("request-extension-client.json"),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "server rejected the extension: {body}"
        );

        let resp: SyncResp = serde_json::from_value(body).unwrap();
        assert_eq!(resp.cards.len(), 1);
        assert_eq!(
            resp.drill_counts.get("1467640::recognition"),
            Some(&1),
            "an integer reviewed_ms must be accepted like a float one"
        );
        let stored = db::test_support::deleted_at(&h.db, EMAIL, "1467640::recall")
            .await
            .expect("tombstone recorded");
        assert_eq!(
            stored, 1_757_000_000_123,
            "a bare integer deleted_at must land as the same instant a float does"
        );
    }

    #[tokio::test]
    async fn a_pre_upgrade_client_body_round_trips() {
        // No `updated_ms` on the cards and bare-string deletions. Such a client
        // must keep syncing rather than 400 on every request.
        let h = Harness::new().await;
        let (status, body) = h
            .post(
                "/api/sync",
                Some(&h.token),
                &fixture("request-legacy-client.json"),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "server rejected a legacy client: {body}"
        );

        let resp: SyncResp = serde_json::from_value(body).unwrap();
        let c = resp.card("1467640::recognition").expect("card stored");
        // `normalize_version` derives a usable version from the times a legacy
        // client does send, so the card can still win or lose a merge.
        assert_eq!(c.updated_ms, 1_756_000_000_000.0);
        assert_eq!(resp.deletions, vec!["1467640::recall".to_string()]);
    }

    #[tokio::test]
    async fn a_resent_review_log_is_counted_once() {
        // A client whose sync response was lost re-sends the same outbox.
        let h = Harness::new().await;
        for _ in 0..2 {
            h.post(
                "/api/sync",
                Some(&h.token),
                &fixture("request-app-client.json"),
            )
            .await;
        }
        let (_, body) = h
            .post("/api/sync", Some(&h.token), &json!({ "cards": [] }))
            .await;
        let resp: SyncResp = serde_json::from_value(body).unwrap();
        assert_eq!(resp.drill_counts.get("1467640::recognition"), Some(&1));
    }

    #[tokio::test]
    async fn the_response_matches_the_shape_clients_parse() {
        let h = Harness::new().await;
        h.post(
            "/api/sync",
            Some(&h.token),
            &fixture("request-app-client.json"),
        )
        .await;
        let (_, body) = h
            .post("/api/sync", Some(&h.token), &json!({ "cards": [] }))
            .await;

        let expected = fixture("response.json");
        let keys = |v: &Value| {
            let mut k: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(
            keys(&body),
            keys(&expected),
            "response top-level keys drifted"
        );
        assert_eq!(
            keys(&body["cards"][0]),
            keys(&expected["cards"][0]),
            "card field set drifted from what clients deserialize"
        );
        assert!(
            expected["deletions"][0].is_string(),
            "clients parse deletions as bare ids"
        );
        assert_eq!(
            keys(&body["cursor"]),
            keys(&expected["cursor"]),
            "cursor shape drifted"
        );
        assert_eq!(body["full"], json!(true), "a body without `since` gets a full sync");
    }

    #[tokio::test]
    async fn a_delta_response_has_the_same_shape_as_a_full_one() {
        let h = Harness::new().await;
        let (_, first) = h
            .post(
                "/api/sync",
                Some(&h.token),
                &fixture("request-app-client.json"),
            )
            .await;
        let (_, body) = h
            .post(
                "/api/sync",
                Some(&h.token),
                &json!({ "cards": [], "since": first["cursor"] }),
            )
            .await;
        let expected = fixture("response-delta.json");
        let keys = |v: &Value| {
            let mut k: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&body), keys(&expected), "delta top-level keys drifted");
        assert_eq!(body["full"], json!(false));
        assert_eq!(expected["full"], json!(false));
    }

    #[tokio::test]
    async fn both_clients_delta_bodies_are_accepted() {
        // The fixtures' cursor names an epoch this fresh database never had,
        // so the server must fall back to a full answer rather than trust it.
        for name in [
            "request-delta-app-client.json",
            "request-delta-extension-client.json",
        ] {
            let h = Harness::new().await;
            let (status, body) = h.post("/api/sync", Some(&h.token), &fixture(name)).await;
            assert_eq!(status, StatusCode::OK, "server rejected {name}: {body}");
            assert_eq!(body["full"], json!(true), "{name}: foreign epoch must go full");
            let resp: SyncResp = serde_json::from_value(body).unwrap();
            assert_eq!(resp.cards.len(), 1, "{name}");
            assert_eq!(resp.drill_counts.get("1467640::recognition"), Some(&1), "{name}");
        }
    }
}

/// Delta sync: clients send only what they changed plus their cursor, and get
/// back only what changed after it — plus the server's ruling on every id they
/// sent. Each scenario builds the one situation where a pure "rows since the
/// cursor" answer would leave two devices permanently apart.
mod delta {
    use super::*;
    use std::collections::HashSet;

    /// A client the way the real ones behave once they hold a cursor: uploads
    /// its dirty cards and pending tombstones, and merges the answer instead
    /// of replacing its deck with it.
    struct DeltaDevice {
        cards: HashMap<String, db::Card>,
        dirty: HashSet<String>,
        tombstones: Vec<(String, f64)>,
        cursor: Option<Value>,
    }

    struct Answer {
        full: bool,
        cards: Vec<String>,
        deletions: Vec<String>,
        drill_counts: HashMap<String, u32>,
    }

    impl DeltaDevice {
        fn new(cards: &[db::Card]) -> Self {
            Self {
                cards: cards.iter().map(|c| (c.id.clone(), c.clone())).collect(),
                dirty: cards.iter().map(|c| c.id.clone()).collect(),
                tombstones: Vec::new(),
                cursor: None,
            }
        }

        fn put(&mut self, c: db::Card) {
            self.dirty.insert(c.id.clone());
            self.cards.insert(c.id.clone(), c);
        }

        fn edit(&mut self, id: &str, f: impl FnOnce(&mut db::Card)) {
            let mut c = self.cards.get(id).expect("card to edit").clone();
            f(&mut c);
            self.put(c);
        }

        fn delete(&mut self, id: &str, at: f64) {
            self.cards.remove(id);
            self.dirty.remove(id);
            self.tombstones.push((id.to_string(), at));
        }

        fn has(&self, id: &str) -> bool {
            self.cards.contains_key(id)
        }

        fn get(&self, id: &str) -> &db::Card {
            self.cards.get(id).unwrap_or_else(|| panic!("no card {id}"))
        }

        /// One sync as the real clients run it: a full answer to a request
        /// that carried a cursor means the server couldn't serve a delta, so
        /// the device drops its cursor and syncs again with its whole deck.
        async fn sync_with(&mut self, h: &Harness, reviews: Value) -> Answer {
            let sent_cursor = self.cursor.is_some();
            let answer = self.sync_once(h, reviews).await;
            if sent_cursor && answer.full {
                self.cursor = None;
                return self.sync_once(h, json!([])).await;
            }
            answer
        }

        async fn sync_once(&mut self, h: &Harness, reviews: Value) -> Answer {
            let mut upload: Vec<&db::Card> = match self.cursor {
                Some(_) => self.dirty.iter().filter_map(|id| self.cards.get(id)).collect(),
                None => self.cards.values().collect(),
            };
            upload.sort_by(|a, b| a.id.cmp(&b.id));
            let mut body = json!({
                "cards": upload,
                "deletions": self.tombstones
                    .iter()
                    .map(|(id, at)| json!({ "id": id, "deleted_at": at }))
                    .collect::<Vec<_>>(),
                "reviews": reviews,
            });
            if let Some(c) = &self.cursor {
                body["since"] = c.clone();
            }
            let (status, body) = h.post("/api/sync", Some(&h.token), &body).await;
            assert_eq!(status, StatusCode::OK, "sync rejected: {body}");
            let full = body["full"].as_bool().expect("full flag");
            let resp: SyncResp = serde_json::from_value(body.clone()).unwrap();
            // Merged, never replaced, as in the real clients: a full answer
            // may come from a server that lost rows this device still holds.
            for c in &resp.cards {
                self.cards.insert(c.id.clone(), c.clone());
            }
            for id in &resp.deletions {
                self.cards.remove(id);
            }
            self.dirty.clear();
            self.tombstones.clear();
            self.cursor = if self.cursor.is_some() && full {
                None
            } else {
                Some(body["cursor"].clone())
            };
            let mut cards: Vec<String> = resp.cards.iter().map(|c| c.id.clone()).collect();
            cards.sort();
            Answer {
                full,
                cards,
                deletions: resp.deletions,
                drill_counts: resp.drill_counts,
            }
        }

        async fn sync(&mut self, h: &Harness) -> Answer {
            self.sync_with(h, json!([])).await
        }
    }

    const Y: &str = "1467640::recall";

    #[tokio::test]
    async fn after_the_first_sync_only_changes_cross_the_wire() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0), card(Y, T0, T0)]);
        let mut b = DeltaDevice::new(&[]);
        assert!(a.sync(&h).await.full, "no cursor yet: full");
        assert!(b.sync(&h).await.full);
        assert!(b.has(X) && b.has(Y));

        let quiet = b.sync(&h).await;
        assert!(!quiet.full);
        assert!(quiet.cards.is_empty() && quiet.deletions.is_empty());

        a.edit(X, |c| {
            c.reps = 1;
            c.updated_ms = T0 + SECOND;
        });
        let own = a.sync(&h).await;
        assert_eq!(own.cards, vec![X.to_string()], "only the edited card is echoed");

        let pulled = b.sync(&h).await;
        assert_eq!(pulled.cards, vec![X.to_string()], "only the edited card is pulled");
        assert_eq!(b.get(X).reps, 1);
        assert_eq!(b.get(Y).reps, 0, "untouched card kept");
    }

    #[tokio::test]
    async fn a_delete_reaches_the_other_device_through_a_delta() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0)]);
        let mut b = DeltaDevice::new(&[]);
        a.sync(&h).await;
        b.sync(&h).await;

        a.delete(X, T0 + SECOND);
        a.sync(&h).await;
        let pulled = b.sync(&h).await;
        assert!(!pulled.full);
        assert_eq!(pulled.deletions, vec![X.to_string()]);
        assert!(!b.has(X));
    }

    #[tokio::test]
    async fn an_upload_that_loses_the_merge_gets_the_winner_back() {
        // A device whose clock runs behind edits a card it already holds the
        // newer copy of. The server keeps its row, which predates the cursor:
        // only the echo tells the device it lost.
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0)]);
        let mut b = DeltaDevice::new(&[]);
        a.sync(&h).await;
        b.sync(&h).await;

        b.edit(X, |c| {
            c.reps = 7;
            c.updated_ms = T0 + 10.0 * SECOND;
        });
        b.sync(&h).await;
        a.sync(&h).await;
        assert_eq!(a.get(X).reps, 7);

        a.edit(X, |c| {
            c.reps = 1;
            c.updated_ms = T0 + 5.0 * SECOND;
        });
        let answer = a.sync(&h).await;
        assert!(!answer.full);
        assert_eq!(answer.cards, vec![X.to_string()], "losing upload must be answered");
        assert_eq!(a.get(X).reps, 7, "A must converge on the server's copy");
        b.sync(&h).await;
        assert_eq!(b.get(X).reps, 7);
    }

    #[tokio::test]
    async fn a_stale_card_refused_by_an_old_tombstone_is_deleted_on_the_device() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0)]);
        a.sync(&h).await;
        a.delete(X, T0 + SECOND);
        a.sync(&h).await;

        // B is up to date (its cursor is past the tombstone) yet somehow still
        // holds the card, and uploads an edit of it.
        let mut b = DeltaDevice::new(&[]);
        b.sync(&h).await;
        b.put(card(X, T0, T0 + 2.0 * SECOND));
        let answer = b.sync(&h).await;
        assert!(!answer.full);
        assert_eq!(answer.deletions, vec![X.to_string()]);
        assert!(!b.has(X));
        assert!(h.stored_ids().await.is_empty());
    }

    #[tokio::test]
    async fn a_delete_refused_by_a_later_re_add_gives_the_card_back() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0)]);
        let mut b = DeltaDevice::new(&[]);
        a.sync(&h).await;
        b.sync(&h).await;

        // B deletes, then re-adds; A pulls the re-added card.
        b.delete(X, T0 + SECOND);
        b.sync(&h).await;
        b.put(card(X, T0 + 3.0 * SECOND, T0 + 3.0 * SECOND));
        b.sync(&h).await;
        a.sync(&h).await;
        assert_eq!(a.get(X).added_ms, T0 + 3.0 * SECOND);

        // A, its clock behind, deletes "before" the re-add. The server keeps
        // the re-add, whose revision A's cursor is already past.
        a.delete(X, T0 + 2.0 * SECOND);
        let answer = a.sync(&h).await;
        assert!(!answer.full);
        assert_eq!(answer.cards, vec![X.to_string()]);
        assert!(a.has(X), "the refused delete must not lose A's card");
        assert_eq!(h.stored_ids().await, vec![X.to_string()]);
    }

    #[tokio::test]
    async fn drill_counts_in_a_delta_cover_changed_and_resent_cards() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0), card(Y, T0, T0)]);
        let drill = |id: &str, at: f64| json!({ "card_id": id, "rating": 1, "kind": "drill", "reviewed_ms": at });
        a.sync_with(&h, json!([drill(X, T0), drill(Y, T0)])).await;

        let answer = a.sync_with(&h, json!([drill(X, T0 + SECOND)])).await;
        assert!(!answer.full);
        assert_eq!(answer.drill_counts.get(X), Some(&2), "a full total, not the increment");
        assert_eq!(answer.drill_counts.get(Y), None, "unchanged counts are left out");

        // A retried upload: the row is already stored under an old revision.
        let answer = a.sync_with(&h, json!([drill(X, T0 + SECOND)])).await;
        assert_eq!(answer.drill_counts.get(X), Some(&2));
    }

    #[tokio::test]
    async fn an_unusable_cursor_falls_back_to_a_full_sync() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0)]);
        a.sync(&h).await;
        let epoch = a.cursor.as_ref().unwrap()["epoch"].clone();

        for (why, cursor) in [
            ("foreign epoch", json!({ "epoch": "not-this-db.1", "rev": 1 })),
            ("cursor ahead of the server", json!({ "epoch": epoch, "rev": 1_000 })),
        ] {
            a.cursor = Some(cursor);
            let answer = a.sync_once(&h, json!([])).await;
            assert!(answer.full, "{why}");
            assert_eq!(answer.cards, vec![X.to_string()], "{why}");
            assert!(a.cursor.is_none(), "{why}: a refused cursor must not be replaced");
        }
    }

    #[tokio::test]
    async fn a_cursor_older_than_pruned_tombstones_falls_back_to_a_full_sync() {
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0), card(Y, T0, T0)]);
        let mut b = DeltaDevice::new(&[]);
        a.sync(&h).await;
        b.sync(&h).await;

        a.delete(Y, T0 + SECOND);
        a.sync(&h).await;
        db::prune_old_deletions(&h.db, i64::MAX).await.unwrap();

        // B never saw the delete, and the tombstone that carried it is gone.
        let answer = b.sync_once(&h, json!([])).await;
        assert!(answer.full, "B's cursor predates a pruned tombstone");
        assert_eq!(answer.cards, vec![X.to_string()]);
        assert!(answer.deletions.is_empty(), "the pruned delete is gone for good");

        assert!(!a.sync(&h).await.full, "A's cursor is past the prune");
    }

    #[tokio::test]
    async fn a_server_that_lost_its_cards_is_repopulated_by_a_delta_client() {
        // A backup restore, or a cards-table drop on a schema break: the
        // server's rows are gone or stale and the epoch is rotated. A device
        // that only uploaded its dirty cards would leave the rest missing for
        // good; the full answer to its cursor is what makes it send them all.
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0), card(Y, T0, T0)]);
        a.sync(&h).await;
        a.sync(&h).await;
        assert!(a.cursor.is_some());

        sqlx::query("DELETE FROM cards").execute(&h.db).await.unwrap();
        sqlx::query("UPDATE sync_meta SET epoch = 'rotated'")
            .execute(&h.db)
            .await
            .unwrap();

        let answer = a.sync(&h).await;
        assert!(answer.full);
        assert_eq!(h.stored_ids().await, vec![Y.to_string(), X.to_string()]);
        assert!(a.has(X) && a.has(Y), "the empty full answer must not wipe the device");
        assert!(!a.sync(&h).await.full, "back on deltas afterwards");
    }

    #[tokio::test]
    async fn another_accounts_cursor_is_not_served_a_delta() {
        // A sign-in raced a sync still in flight for the previous account, and
        // that sync stored its cursor after the sign-in cleared it.
        let h = Harness::new().await;
        let mut a = DeltaDevice::new(&[card(X, T0, T0)]);
        a.sync(&h).await;
        a.sync(&h).await;
        let mut cursor = a.cursor.clone().unwrap();
        let epoch = cursor["epoch"].as_str().unwrap();
        let (db_epoch, user) = epoch.rsplit_once('.').unwrap();
        let other = user.parse::<i64>().unwrap() + 1;
        cursor["epoch"] = json!(format!("{db_epoch}.{other}"));

        a.cursor = Some(cursor);
        assert!(a.sync_once(&h, json!([])).await.full);
    }
}

mod rate_limits {
    use super::*;

    #[tokio::test]
    async fn a_review_session_is_not_throttled_by_the_auth_quota() {
        // One sync per reviewed card, all from one IP. Under the shared 10/min
        // auth quota the 11th was a 429, and clients only retry on the next
        // change — so the end of a session could stay unsynced.
        let h = Harness::with_quotas(crate::auth_quota(), crate::sync_quota()).await;
        for i in 0..30 {
            let (status, body) = h
                .post("/api/sync", Some(&h.token), &json!({ "cards": [] }))
                .await;
            assert_eq!(status, StatusCode::OK, "sync {i} was throttled: {body}");
        }
    }
}
