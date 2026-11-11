//! Settings from the UI's side: the [`yomeru_core::settings`] types plus
//! `load()` / `save()` shims that go through the
//! [`crate::platform::SettingsStore`] in Dioxus context, installed via
//! [`crate::launch_with`].

use dioxus::prelude::consume_context;
use dioxus::prelude::*;

pub use yomeru_core::settings::*;

use crate::platform::Platform;

/// Synchronous read of the current settings off whichever store the
/// platform installed.
pub fn load() -> SrsSettings {
    consume_context::<Platform>().settings.load()
}

/// Saves settings. The save is fire-and-forget on
/// the underlying async store — callers that want backpressure should use
/// the trait directly.
pub fn save(s: &SrsSettings) -> Result<(), String> {
    let platform = consume_context::<Platform>();
    let s_owned = s.clone();
    crate::rt::spawn_detached(async move {
        if let Err(e) = platform.settings.save(s_owned).await {
            warn!("settings save failed: {e}");
        }
    });
    Ok(())
}
