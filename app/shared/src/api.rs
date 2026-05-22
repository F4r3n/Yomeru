//! Auth shims that delegate to the [`crate::platform::SettingsStore`] in
//! Dioxus context. The HTTP implementation lives in `platform.rs`; the
//! extension provides its own implementation that messages the background
//! script.
//!
//! `sync_cards` is no longer surfaced as a free function — sync is owned
//! by the SettingsStore (`schedule_sync` / `sync_now`).

use dioxus::prelude::use_context;

use crate::platform::Platform;

/// In dev mode the server auto-issues a session and returns the token
/// directly in the response body — callers should treat `Ok(Some(token))`
/// as "already authenticated, skip the OTP step".
pub async fn request_otp(server_url: &str, email: &str) -> Result<Option<String>, String> {
    use_context::<Platform>()
        .settings
        .request_otp(server_url, email)
        .await
}

pub async fn verify_otp(
    server_url: &str,
    email: &str,
    code: &str,
) -> Result<String, String> {
    use_context::<Platform>()
        .settings
        .verify_otp(server_url, email, code)
        .await
}
