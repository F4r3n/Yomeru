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
"#)]
extern "C" {
    pub fn send_message(msg: JsValue) -> Promise;
    pub fn storage_get(key: &str) -> Promise;
    pub fn add_storage_listener(cb: &Function);
}
