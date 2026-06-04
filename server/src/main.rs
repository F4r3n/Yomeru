use std::{net::IpAddr, net::SocketAddr, num::NonZeroU32, sync::Arc};

use anyhow::Context;
use axum::{routing::post, Router};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod api;
mod config;
mod db;
mod dicts;

use config::Config;
use db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    pub limiter: Arc<DefaultKeyedRateLimiter<IpAddr>>,
    pub lookup_limiter: Arc<DefaultKeyedRateLimiter<IpAddr>>,
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

    let cfg = Arc::new(Config::from_args());
    if cfg.dev_mode {
        warn!("DEV MODE — SMTP skipped, /api/sync auth disabled");
    }

    let db = db::init_db(&cfg.db_path).await.context("init db")?;
    {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // 90 days
        db::prune_old_deletions(&db, now_ms - 90 * 86_400_000)
            .await
            .context("prune old deletions at startup")?;
    }

    dicts::init_all(&cfg.data_dir)
        .with_context(|| format!("load dict data from {}", cfg.data_dir))?;
    info!(data_dir = %cfg.data_dir, "dict data loaded");

    // Auth endpoints use a strict per-minute quota; lookup uses a separate
    // higher-rate limiter (one keystroke = one lookup, easily 30+/min).
    // `const { ... }` evaluates at compile time, so the `unwrap` here cannot
    // ever panic at runtime — the value is materialized when the binary builds.
    let quota = Quota::per_minute(const { NonZeroU32::new(10).unwrap() });
    let limiter = Arc::new(RateLimiter::keyed(quota));
    let lookup_quota = Quota::per_second(const { NonZeroU32::new(20).unwrap() });
    let lookup_limiter = Arc::new(RateLimiter::keyed(lookup_quota));

    let state = AppState {
        db,
        cfg: cfg.clone(),
        limiter,
        lookup_limiter,
    };

    let app = Router::new()
        .route("/api/auth/request", post(api::auth_request_handler))
        .route("/api/auth/verify", post(api::auth_verify_handler))
        .route("/api/sync", post(api::sync_handler))
        .route("/api/lookup", post(api::lookup_handler))
        .route("/api/lookup-by-sequence", post(api::lookup_by_sequence_handler))
        .route("/api/lookup-prefix", post(api::lookup_prefix_handler))
        .route("/api/kanji", post(api::kanji_handler))
        .route("/api/examples", post(api::examples_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
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
