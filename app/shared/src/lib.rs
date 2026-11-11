//! The Yomeru UI (Dioxus), shared by the website, the extension and the
//! Android app. The logic underneath is `yomeru-core`; its modules are
//! re-exported here so routes keep writing `crate::types::…` and the entry
//! crates `yomeru_shared::platform::…`. The modules defined here wrap core
//! functions that need the [`Platform`] with versions that read it from
//! Dioxus context.

pub use yomeru_core::{idb, platform, romaji, srs, types};

pub mod api;
pub mod cards_io;
pub mod dict;
pub mod drill_log;
pub mod icons;
pub mod kanji_deck;
/// Offline dictionary for native builds that bundle the data.
#[cfg(not(target_arch = "wasm32"))]
pub mod local_dict;
pub mod rt;
pub mod settings;
pub mod sync;
pub mod theme;

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
    // Natively, core has no task runner of its own: detached tasks (auto-sync,
    // settings saves) run on the root scope so leaving a page can't cancel
    // them.
    #[cfg(not(target_arch = "wasm32"))]
    yomeru_core::rt::set_spawner(|fut| {
        dioxus::core::spawn_forever(fut);
    });
    yomeru_core::sync::set_observer(Box::new(sync::SignalObserver));
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
