pub mod api;
pub mod cards_io;
pub mod dict;
pub mod drill_log;
pub mod icons;
/// The card store: IndexedDB in the browser, SQLite natively. Same API.
#[cfg(target_arch = "wasm32")]
pub mod idb;
#[cfg(not(target_arch = "wasm32"))]
#[path = "native/store.rs"]
pub mod idb;
/// Offline dictionary for native builds that bundle the data.
#[cfg(not(target_arch = "wasm32"))]
#[path = "native/dict.rs"]
pub mod local_dict;
pub mod kanji_deck;
pub mod platform;
pub mod romaji;
pub mod rt;
pub mod settings;
pub mod srs;
mod store_common;
pub mod sync;
pub mod theme;
pub mod types;

mod app;
mod components;
mod routes;

pub use app::App;
pub use platform::{Platform, default_http_platform, platform_with_dict};

use std::cell::RefCell;

use dioxus::prelude::*;

// Stashed before `dioxus::launch` because that API takes a `fn() ->
// Element` (no captures). The wrapper component below pulls it out on
// first render and installs it via `use_context_provider`.
thread_local! {
    static PENDING_PLATFORM: RefCell<Option<Platform>> = const { RefCell::new(None) };
}

/// Mounts the app with the given platform installed via Dioxus context.
/// `routes/*` reach the platform implicitly through `use_context::<Platform>()`
/// inside their `dict::lookup` / `settings::load` / `sync::schedule_sync`
/// call sites.
///
/// Used by `app/web/src/main.rs`, `app/android/src/main.rs`, and the new
/// `app/extension/src/main.rs` — each builds a `Platform` appropriate for
/// its target.
pub fn launch_with(platform: Platform) {
    PENDING_PLATFORM.with(|p| *p.borrow_mut() = Some(platform));
    dioxus::launch(launched_app);
}

fn launched_app() -> Element {
    use_context_provider(|| {
        // Structural invariant: `launch_with` is the only caller and it
        // installs PENDING_PLATFORM before `dioxus::launch`. A None here
        // would mean someone called `dioxus::launch(launched_app)` directly.
        #[allow(clippy::expect_used)]
        PENDING_PLATFORM
            .with(|p| p.borrow_mut().take())
            .expect("launch_with must install a Platform before launching")
    });
    rsx! { App {} }
}
