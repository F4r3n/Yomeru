use js_sys::{Function, Promise};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function send_message(msg) {
    return browser.runtime.sendMessage(msg);
}
export function storage_get(key) {
    return browser.storage.local.get(key);
}
export function storage_set_enabled(value) {
    return browser.storage.local.set({ enabled: value });
}
export function add_storage_listener(cb) {
    browser.storage.onChanged.addListener(function(changes, area) {
        if (area === "local") { cb(changes); }
    });
}
// What sync sends off the device, as Firefox data-consent categories. Keep in
// step with `optional` in manifest.json and SYNC_DATA_COLLECTION in
// extension/src/background/consent.ts.
const SYNC_DATA = ["personallyIdentifyingInfo", "authenticationInfo"];
// Firefox shows the prompt only while handling a click, so this has to be
// reached from the click's handler with nothing awaited before it. Resolves
// to whether the user granted it.
export function request_sync_consent() {
    return browser.permissions.request({ data_collection: SYNC_DATA }).catch(function(e) {
        console.warn("[yomeru] data consent request failed:", e);
        return false;
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
    pub fn storage_set_enabled(value: bool) -> Promise;
    pub fn add_storage_listener(cb: &Function);
    pub fn navigate_to_root();
    pub fn request_sync_consent() -> Promise;
}
