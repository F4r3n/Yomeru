use std::{net::IpAddr, net::SocketAddr, num::NonZeroU32, sync::Arc};

use axum::{routing::post, Router};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use tower_http::cors::CorsLayer;

mod api;
mod config;
mod db;

use config::Config;
use db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    pub limiter: Arc<DefaultKeyedRateLimiter<IpAddr>>,
}

#[tokio::main]
async fn main() {
    let cfg = Arc::new(Config::from_args());

    let db = db::init_db(&cfg.db_path);

    let quota = Quota::per_minute(NonZeroU32::new(10).unwrap());
    let limiter = Arc::new(RateLimiter::keyed(quota));

    let state = AppState { db, cfg: cfg.clone(), limiter };

    let app = Router::new()
        .route("/api/auth/request", post(api::auth_request_handler))
        .route("/api/auth/verify", post(api::auth_verify_handler))
        .route("/api/sync", post(api::sync_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    println!("[yomeru-server] listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
