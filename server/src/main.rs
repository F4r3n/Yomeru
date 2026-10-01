use std::{net::IpAddr, net::SocketAddr, num::NonZeroU32, sync::Arc, time::Duration};

use anyhow::Context;
use axum::http::HeaderMap;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod api;
mod client_ip;
mod config;
mod db;
mod dicts;

use config::Config;
use db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    /// OTP request/verify: strict, since it guards code guessing.
    pub limiter: Arc<DefaultKeyedRateLimiter<IpAddr>>,
    pub sync_limiter: Arc<DefaultKeyedRateLimiter<IpAddr>>,
    pub lookup_limiter: Arc<DefaultKeyedRateLimiter<IpAddr>>,
}

// `const { ... }` evaluates at compile time, so the `unwrap`s below cannot ever
// panic at runtime — the values are materialized when the binary builds.

/// Auth endpoints: a strict per-minute quota.
pub(crate) fn auth_quota() -> Quota {
    Quota::per_minute(const { NonZeroU32::new(10).unwrap() })
}

/// `/api/sync`. Clients sync 2 s after every card change, so a steady review
/// session alone can sustain one request every few seconds — and every device
/// behind one NAT shares the key. Sharing the auth quota (10/min) 429'd those
/// sessions, and clients don't retry a failed sync until the next change.
pub(crate) fn sync_quota() -> Quota {
    Quota::per_minute(const { NonZeroU32::new(60).unwrap() })
}

/// Lookups: one keystroke = one lookup, easily 30+/min.
pub(crate) fn lookup_quota() -> Quota {
    Quota::per_second(const { NonZeroU32::new(20).unwrap() })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl AppState {
    /// Rate-limiter key for a request: the real client address, not the peer
    /// (which is the reverse proxy in any fronted deployment).
    pub fn client_ip(&self, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
        client_ip::client_ip(
            peer.ip(),
            headers,
            self.cfg.trust_proxy,
            self.cfg.proxy_hops,
        )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Default to info; honor RUST_LOG when set (e.g. `RUST_LOG=server=debug,tower_http=debug`).
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,axum::rejection=trace"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();

    // Best-effort .env load for local dev. Containers set env vars directly
    // so the file isn't required; ignore "not found".
    match dotenvy::dotenv() {
        Ok(path) => info!(path = %path.display(), "loaded env file"),
        Err(e) if e.not_found() => {}
        Err(e) => warn!(error = %e, ".env load warning"),
    }

    let cfg = Arc::new(Config::from_args()?);
    if cfg.dev_mode {
        warn!(
            bind = %cfg.bind,
            "DEV MODE — /api/auth/request issues a session token for ANY email \
             with no OTP. Never enable this on a public deployment."
        );
    }
    info!(
        trust_proxy = ?cfg.trust_proxy,
        hops = cfg.proxy_hops,
        "rate-limit client-IP resolution"
    );

    let db = db::init_db(&cfg.db_path).await.context("init db")?;
    {
        let now = now_ms();
        // 90 days
        db::prune_old_deletions(&db, now - 90 * 86_400_000)
            .await
            .context("prune old deletions at startup")?;
        db::prune_expired_auth(&db, now)
            .await
            .context("prune expired auth rows at startup")?;
    }

    // Sessions last 30 days and OTPs 10 minutes, so a long-lived process would
    // otherwise accumulate dead rows indefinitely between restarts.
    tokio::spawn({
        let db = db.clone();
        async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(6 * 3_600));
            ticker.tick().await; // fires immediately; startup already pruned
            loop {
                ticker.tick().await;
                if let Err(e) = db::prune_expired_auth(&db, now_ms()).await {
                    warn!(error = ?e, "periodic auth prune failed");
                }
            }
        }
    });

    dicts::init_all(&cfg.data_dir)
        .with_context(|| format!("load dict data from {}", cfg.data_dir))?;
    info!(data_dir = %cfg.data_dir, "dict data loaded");

    let state = AppState {
        db,
        cfg: cfg.clone(),
        limiter: Arc::new(RateLimiter::keyed(auth_quota())),
        sync_limiter: Arc::new(RateLimiter::keyed(sync_quota())),
        lookup_limiter: Arc::new(RateLimiter::keyed(lookup_quota())),
    };

    let app = api::router(state);

    let addr = format!("{}:{}", cfg.bind, cfg.port);
    info!(%addr, "listening");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("axum::serve")?;
    Ok(())
}
