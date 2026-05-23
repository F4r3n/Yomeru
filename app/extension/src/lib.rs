use std::rc::Rc;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

mod bridge;
mod dict_msg;
mod settings_msg;

#[wasm_bindgen(start)]
pub async fn main() {
    console_error_panic_hook::set_once();
    if let Err(e) = settings_msg::hydrate().await {
        log::warn!("settings hydration failed: {e}");
    }
    settings_msg::register_storage_watcher();
    let platform = yomeru_shared::Platform {
        dict: Rc::new(dict_msg::ExtensionDict),
        settings: Rc::new(settings_msg::ExtensionSettings),
    };
    yomeru_shared::launch_with(platform);
}

/// Serialize `payload` and send `{ type, payload }` to the background
/// script via `browser.runtime.sendMessage`, then deserialize the response
/// as `R`.
pub(crate) async fn send_bg_message<P, R>(ty: &str, payload: P) -> Result<R, String>
where
    P: Serialize,
    R: for<'de> Deserialize<'de>,
{
    #[derive(Serialize)]
    struct Msg<'a, P: Serialize> {
        #[serde(rename = "type")]
        ty: &'a str,
        payload: P,
    }
    let js_msg = serde_wasm_bindgen::to_value(&Msg { ty, payload }).map_err(|e| e.to_string())?;
    let result = JsFuture::from(bridge::send_message(js_msg))
        .await
        .map_err(|e| format!("{e:?}"))?;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}
