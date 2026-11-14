//! JMdict XML parser, shared by the `jmdict-build` and `examples-build` CLIs.
// The byte scanners in `parser` use bounds-guarded indexing on the input XML buffer.
#![allow(clippy::indexing_slicing)]

pub mod parser;
