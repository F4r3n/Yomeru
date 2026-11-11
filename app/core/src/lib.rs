//! The Yomeru app's logic, with no UI: card store, SRS scheduling, sync,
//! settings, dictionary clients, card import and the kanji deck.
//!
//! Nothing here depends on Dioxus. The UI (`yomeru-shared`) builds a
//! [`platform::Platform`] and passes it to the functions that need the
//! dictionary or the settings store; it also installs, once at startup, the
//! task spawner ([`rt::set_spawner`], native only) and the sync status sink
//! ([`sync::set_observer`]).

pub mod cards_io;
pub mod dict;
pub mod drill_log;
/// The card store: IndexedDB in the browser, SQLite natively. Same API.
#[cfg(target_arch = "wasm32")]
pub mod idb;
#[cfg(not(target_arch = "wasm32"))]
#[path = "native/store.rs"]
pub mod idb;
pub mod kanji_deck;
/// Offline dictionary for native builds that bundle the data.
#[cfg(not(target_arch = "wasm32"))]
#[path = "native/dict.rs"]
pub mod local_dict;
pub mod platform;
pub mod romaji;
pub mod rt;
pub mod settings;
pub mod srs;
mod store_common;
pub mod sync;
pub mod types;
