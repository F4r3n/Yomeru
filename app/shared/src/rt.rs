//! [`yomeru_core::rt`]'s host services, plus the one that needs the UI: a
//! confirm prompt.

pub use yomeru_core::rt::*;

/// The browser's own `confirm()`.
#[cfg(target_arch = "wasm32")]
pub async fn confirm(message: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
}

/// The WebView's own `confirm()`. Dioxus mobile allows `eval`, unlike the
/// extension's CSP.
#[cfg(not(target_arch = "wasm32"))]
pub async fn confirm(message: &str) -> bool {
    let Ok(msg) = serde_json::to_string(message) else {
        return false;
    };
    let mut eval = dioxus::document::eval(&format!("dioxus.send(window.confirm({msg}));"));
    eval.recv::<bool>().await.unwrap_or(false)
}
