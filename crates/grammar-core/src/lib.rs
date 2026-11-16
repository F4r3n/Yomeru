//! Grammar dictionary: Yomeru's own `data/grammar.toml`, compiled into
//! `grammar.bin` at build time (feature `build`) and shipped with the app
//! like JMdict, then loaded and looked up in text here.

#[cfg(feature = "build")]
mod compile;
mod dict;
mod format;
pub mod types;

#[cfg(all(test, feature = "build"))]
mod tests;

#[cfg(feature = "build")]
pub use compile::{Compiled, compile};
pub use dict::{GrammarDict, HOVER_MIN_CHARS};
pub use format::FORMAT_VERSION;
pub use types::{Example, GrammarEntry, Related, Rich, Span};
