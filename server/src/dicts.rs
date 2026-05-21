//! Loads jmdict.bin / kanjidic.bin / examples.bin at startup. The underlying
//! crates store their parsed state in process-global `OnceCell`s, so once
//! `init_all` returns successfully the lookup fns can be called from any
//! handler without further plumbing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn init_all(data_dir: &str) -> Result<()> {
    let dir = PathBuf::from(data_dir);
    init_jmdict(&dir.join("jmdict.bin"))?;
    init_kanjidic(&dir.join("kanjidic.bin"))?;
    init_examples(&dir.join("examples.bin"))?;
    Ok(())
}

fn init_jmdict(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    jmdict_wasm::init_for_testing(&bytes).context("init jmdict")
}

fn init_kanjidic(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    kanjidic_wasm::init_from_bytes(&bytes).context("init kanjidic")
}

fn init_examples(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    examples_wasm::init_from_bytes(&bytes).context("init examples")
}
