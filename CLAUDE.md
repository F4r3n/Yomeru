# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Full rebuild: download dict (if missing) → build binary index → compile WASM modules
cargo xtask build-all

# Rebuild WASM modules only (fast iteration, dev profile)
cargo xtask build --profile dev

# Rebuild binary dictionary index from existing JMdict_e XML
cargo xtask build-dict --input JMdict_e

# Download JMdict_e.gz from EDRDG and decompress (skips if already present)
cargo xtask download-dict

# Run tests for all host crates (excludes WASM crates which require wasm-pack)
cargo xtask test

# Run tests for a single crate
cargo test -p deinflect

# Run all benchmarks (deinflect + japanese-utils)
cargo bench -p deinflect -p japanese-utils

# Run jmdict-core benchmarks (requires test-utils feature)
cargo bench -p jmdict-core --features test-utils

# Build extension TypeScript/Svelte (requires node + npm)
cargo xtask build-js
# or directly:
cd extension && npm install && npm run build
```

## Crate map

| Crate | Role |
|---|---|
| `japanese-utils` | Char classification, `extract_japanese_run(text, char_offset)` — returns from offset forward |
| `jmdict-types` | `WordEntry`, `Sense`, `PartOfSpeech` — shared serde types |
| `jmdict-build` | Offline CLI: JMdict XML → binary index; lookup table stores **byte offsets**, not element indices |
| `deinflect` | BFS suffix-replacement, depth ≤ 3; returns `Vec<Deinflected { text, reason }>` |
| `jmdict-core` | Pure-Rust JMdict runtime: `init`, `lookup`, `lookup_longest_match` (returns `(entries, match_len)` where `match_len` = chars in surface form), `lookup_prefix`, `find_in_text` |
| `jmdict-wasm` | Thin `#[wasm_bindgen]` shim over `jmdict-core` (exposes the `Dictionary` JS class) |
| `kanjidic-core` / `kanjidic-wasm` | Same core/shim split: core exposes `init_from_bytes`, `lookup_one`, `lookup_many`; wasm adds the JS class |
| `examples-core` / `examples-wasm` | Same split for example sentences |
| `srs-core` | Stateless SM-2: `new_card`, `review_card`, `filter_due` |
| `srs-wasm` | WASM wrapper for srs-core |

Native consumers (server, app, benches) depend on the `-core` crates. The `-wasm` crates exist only to expose those cores to JavaScript. Cargo crate names match the `extension/_generated/<crate>/<crate>_module.js` paths in the manifest — renaming a `-wasm` crate would also break extension JS imports.

## Binary dictionary format (`jmdict.bin`)

`JMDI` magic + version byte, then three length-prefixed blobs:
1. FST bytes — maps headword/reading → group index
2. Lookup table (postcard `Vec<Vec<u32>>`) — group index → list of **byte offsets** into entries blob
3. Entries blob — each entry is a `u32 LE` length prefix followed by postcard-encoded `WordEntry`

`get_entry(idx)` in `dictionary.rs` treats `idx` as a byte offset into the entries blob.

## Deinflection + lookup flow

`lookup_longest_match(text, max_chars)` iterates char boundaries longest-first. For each prefix it enqueues the surface form and all deinflected candidates (tagged with the prefix's char count). Returns `(Vec<WordEntry>, match_len)` for the first FST hit.
