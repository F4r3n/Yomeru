//! Card import/export from the UI's side: [`yomeru_core::cards_io`] plus the
//! platform's way of handing a file to the user.

use dioxus::prelude::consume_context;

pub use yomeru_core::cards_io::*;

use crate::platform::Platform;

/// Imports a deck exported by [`yomeru_core::cards_io`]'s format, with the
/// platform in Dioxus context.
pub async fn import_cards_json(text: &str) -> Result<(usize, Skips), String> {
    yomeru_core::cards_io::import_cards_json(&consume_context::<Platform>(), text).await
}

/// Hands `content` to the user as a file. Returns where it went, for the
/// status line, when that isn't obvious.
///
/// In the browser: a Blob URL on a transient <a>, the same pattern the
/// extension uses for JSON export; the browser shows its own download UI.
#[cfg(target_arch = "wasm32")]
pub fn trigger_download(content: &str, filename: &str) -> Result<Option<String>, String> {
    use wasm_bindgen::{JsCast, JsValue};
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let blob_parts = js_sys::Array::new();
    blob_parts.push(&JsValue::from_str(content));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/json");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &opts)
        .map_err(|_| "blob create failed".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "url create failed".to_string())?;
    let a = document
        .create_element("a")
        .map_err(|_| "create_element failed".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "anchor cast failed".to_string())?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    web_sys::Url::revoke_object_url(&url).ok();
    Ok(None)
}

/// Natively: written to the shared Download folder, which Android 11+ lets an
/// app add its own files to without a storage permission; failing that (older
/// Android, desktop), into the app's data dir.
#[cfg(not(target_arch = "wasm32"))]
pub fn trigger_download(content: &str, filename: &str) -> Result<Option<String>, String> {
    let candidates = [
        std::path::PathBuf::from("/storage/emulated/0/Download"),
        crate::rt::data_dir().to_path_buf(),
    ];
    let mut last_err = String::from("no writable folder");
    for dir in candidates {
        if !dir.is_dir() {
            continue;
        }
        let path = dir.join(filename);
        match std::fs::write(&path, content) {
            Ok(()) => return Ok(Some(path.display().to_string())),
            Err(e) => last_err = format!("{}: {e}", path.display()),
        }
    }
    Err(last_err)
}
