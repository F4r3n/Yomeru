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

        // Production quota is 10/min on this route and every simulated device
        // shares 127.0.0.1, so a multi-round convergence test would trip it.
        // Rate limiting has its own coverage; here it would only add flake.
        let quota = Quota::per_second(const { NonZeroU32::new(10_000).unwrap() });
        let state = AppState {
            db: db.clone(),
            cfg: Arc::new(cfg),
            limiter: Arc::new(RateLimiter::keyed(quota)),
            lookup_limiter: Arc::new(RateLimiter::keyed(quota)),
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
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
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
}

impl SyncResp {
    fn card(&self, id: &str) -> Option<&db::Card> {
        self.cards.iter().find(|c| c.id == id)
    }
}

// ---- a simulated device -------------------------------------------------

/// One client's local state. Uploads its whole deck plus pending tombstones —
/// which is what the real clients do, and the reason the tombstone guards exist
/// at all — then adopts the server's answer.
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
    assert!(resp.card(X).is_none(), "B's stale copy resurrected the card");
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
    assert!(resp.card(X).is_some(), "A should get the re-added card back");
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
    a.tombstones
        .push((X.to_string(), T0 + 3_650.0 * DAY)); // ~10 years out
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
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse fixture {path}: {e}"))
    }

    #[tokio::test]
    async fn the_rust_client_body_round_trips() {
        let h = Harness::new().await;
        let (status, body) = h
            .post("/api/sync", Some(&h.token), &fixture("request-app-client.json"))
            .await;
        assert_eq!(status, StatusCode::OK, "server rejected the app client: {body}");

        let resp: SyncResp = serde_json::from_value(body).unwrap();
        assert_eq!(resp.cards.len(), 1);
        assert_eq!(resp.deletions, vec!["1467640::recall".to_string()]);
        // Float `deleted_at` must survive as a real time, not collapse to
        // receipt time — that is the whole point of sending it.
        let stored = db::test_support::deleted_at(&h.db, EMAIL, "1467640::recall")
            .await
            .expect("tombstone recorded");
        assert_eq!(stored, 1_757_000_000_123);
    }

    #[tokio::test]
    async fn the_extension_body_round_trips() {
        // Same payload in JavaScript's dialect: integer-valued floats and no
        // `settings` key, since the extension doesn't sync settings.
        let h = Harness::new().await;
        let (status, body) = h
            .post(
                "/api/sync",
                Some(&h.token),
                &fixture("request-extension-client.json"),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "server rejected the extension: {body}");

        let resp: SyncResp = serde_json::from_value(body).unwrap();
        assert_eq!(resp.cards.len(), 1);
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
        assert_eq!(status, StatusCode::OK, "server rejected a legacy client: {body}");

        let resp: SyncResp = serde_json::from_value(body).unwrap();
        let c = resp.card("1467640::recognition").expect("card stored");
        // `normalize_version` derives a usable version from the times a legacy
        // client does send, so the card can still win or lose a merge.
        assert_eq!(c.updated_ms, 1_756_000_000_000.0);
        assert_eq!(resp.deletions, vec!["1467640::recall".to_string()]);
    }

    #[tokio::test]
    async fn the_response_matches_the_shape_clients_parse() {
        let h = Harness::new().await;
        h.post("/api/sync", Some(&h.token), &fixture("request-app-client.json"))
            .await;
        let (_, body) = h
            .post("/api/sync", Some(&h.token), &json!({ "cards": [] }))
            .await;

        let expected = fixture("response.json");
        let keys = |v: &Value| {
            let mut k: Vec<String> = v
                .as_object()
                .expect("object")
                .keys()
                .cloned()
                .collect();
            k.sort();
            k
        };
        assert_eq!(keys(&body), keys(&expected), "response top-level keys drifted");
        assert_eq!(
            keys(&body["cards"][0]),
            keys(&expected["cards"][0]),
            "card field set drifted from what clients deserialize"
        );
        assert!(
            expected["deletions"][0].is_string(),
            "clients parse deletions as bare ids"
        );
    }
}
