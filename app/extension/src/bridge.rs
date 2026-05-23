use js_sys::{Function, Promise};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function send_message(msg) {
    return browser.runtime.sendMessage(msg);
}
export function storage_get(key) {
    return browser.storage.local.get(key);
}
export function add_storage_listener(cb) {
    browser.storage.onChanged.addListener(function(changes, area) {
        if (area === "local") { cb(changes); }
    });
}
// The popup URL is moz-extension://<id>/options.html — its path "/options.html"
// matches no Dioxus route. Push "/" so WebHistory initialises at the root.
export function navigate_to_root() {
    history.pushState(null, '', '/');
}
"#)]
extern "C" {
    pub fn send_message(msg: JsValue) -> Promise;
    pub fn storage_get(key: &str) -> Promise;
    pub fn add_storage_listener(cb: &Function);
    pub fn navigate_to_root();
}
