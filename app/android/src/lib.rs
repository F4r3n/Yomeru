#[cfg(target_os = "android")]
#[no_mangle]
fn start_app() {
    dioxus::launch(yomeru_shared::App);
}
