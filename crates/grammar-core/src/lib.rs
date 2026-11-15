//! Grammar dictionary: converts a Yomitan grammar dictionary ZIP into our
//! own `grammar.bin`, then loads it and looks grammar points up in text.
//!
//! The dictionary's content belongs to its sources, so Yomeru never ships,
//! hosts or syncs it: each device downloads or imports the ZIP and converts
//! it here. Tests use hand-written entries only.

mod convert;
mod dict;
mod format;
pub mod types;

#[cfg(test)]
mod tests;

pub use convert::{Converted, convert};
pub use dict::GrammarDict;
pub use format::FORMAT_VERSION;
pub use types::{
    Block, Example, GrammarEntry, GrammarMeta, ParagraphKind, Rich, Sense, SourceBlock, Span,
};
