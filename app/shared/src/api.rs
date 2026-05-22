//! HTTP client for the yomeru-server endpoints. Mirrors the extension's
//! `handleRequestOtp` / `handleVerifyOtp` / `handleSyncCards` in
//! `src/background/background.ts`.

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};

use crate::types::SrsCard;

#[derive(Serialize)]
struct AuthRequestBody<'a> {
    email: &'a str,
}

#[derive(Serialize)]
struct VerifyBody<'a> {
    email: &'a str,
    code: &'a str,
}

#[derive(Deserialize)]
struct VerifyResponse {
    token: String,
}

#[derive(Serialize)]
struct SyncBody<'a> {
    cards: &'a [SrsCard],
}

#[derive(Deserialize)]
struct SyncResponse {
    cards: Vec<SrsCard>,
}

fn join(base: &str, path: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}{path}")
}

/// In dev mode the server auto-issues a session and returns the token
/// directly in the response body — callers should treat `Ok(Some(token))`
/// as "already authenticated, skip the OTP step".
pub async fn request_otp(server_url: &str, email: &str) -> Result<Option<String>, String> {
    let body = AuthRequestBody { email };
    let res = Request::post(&join(server_url, "/api/auth/request"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    // 204 No Content = normal OTP flow; 200 with {token} = dev-mode auto-auth.
    if res.status() == 204 {
        return Ok(None);
    }
    let parsed: VerifyResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(Some(parsed.token))
}

pub async fn verify_otp(
    server_url: &str,
    email: &str,
    code: &str,
) -> Result<String, String> {
    let body = VerifyBody { email, code };
    let res = Request::post(&join(server_url, "/api/auth/verify"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("server {status}: {text}"));
    }
    let parsed: VerifyResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.token)
}

pub async fn sync_cards(
    server_url: &str,
    token: &str,
    cards: &[SrsCard],
) -> Result<Vec<SrsCard>, String> {
    let body = SyncBody { cards };
    let res = Request::post(&join(server_url, "/api/sync"))
        .header("Authorization", &format!("Bearer {token}"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status() == 401 {
        return Err("session expired — re-verify".into());
    }
    if !res.ok() {
        return Err(format!("server {}", res.status()));
    }
    let parsed: SyncResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.cards)
}
