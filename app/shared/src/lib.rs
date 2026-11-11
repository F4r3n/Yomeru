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

/// Mounts the app with the given platform installed via Dioxus context.
/// `routes/*` reach the platform implicitly through `use_context::<Platform>()`
/// inside their `dict::lookup` / `settings::load` / `sync::schedule_sync`
/// call sites.
///
/// Used by `app/web/src/main.rs` and `app/extension/src/lib.rs`; the Android
/// app goes through [`launch_with_config`]. Each builds a `Platform`
/// appropriate for its target.
pub fn launch_with(platform: Platform) {
    launch_builder(platform).launch(App);
}

/// [`launch_with`], plus a platform-specific Dioxus config: the Android app
/// passes its mobile `Config`, carrying [`startup_head`].
pub fn launch_with_config(platform: Platform, config: impl dioxus::core::LaunchConfig) {
    launch_builder(platform).with_cfg(config).launch(App);
}

pub use app::startup_head;

fn launch_builder(platform: Platform) -> dioxus::LaunchBuilder {
    // Natively, core has no task runner of its own: detached tasks (auto-sync,
    // settings saves) run on the root scope so leaving a page can't cancel
    // them.
    #[cfg(not(target_arch = "wasm32"))]
    yomeru_core::rt::set_spawner(|fut| {
        dioxus::core::spawn_forever(fut);
    });
    yomeru_core::sync::set_observer(Box::new(sync::SignalObserver));
    dioxus::LaunchBuilder::new().with_context(platform)
}
