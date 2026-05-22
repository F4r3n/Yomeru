//! Pure-Rust JMdict runtime: binary dictionary loader + lookup primitives.
//!
//! The `jmdict-wasm` crate is a thin `#[wasm_bindgen]` shim on top of this.
//! Native consumers (server, app, benches) depend on this crate directly.

pub mod dictionary;
pub mod lookup;

#[cfg(test)]
mod tests;

pub use dictionary::{init, init_for_testing};
pub use lookup::{find_in_text, lookup, lookup_longest_match, lookup_prefix};
