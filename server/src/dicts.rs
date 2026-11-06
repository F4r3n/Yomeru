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
    init_overlay(&dir.join("kanji-mnemonics.tsv"), kanjidic_core::init_mnemonics)?;
    init_overlay(&dir.join("part-names.tsv"), kanjidic_core::init_part_names)?;
    init_examples(&dir.join("examples.bin"))?;
    Ok(())
}

fn init_jmdict(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    jmdict_core::init(&bytes).context("init jmdict")
}

fn init_kanjidic(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    kanjidic_core::init_from_bytes(&bytes).context("init kanjidic")
}

/// One of the optional files laid over KANJIDIC (mnemonics, part names).
/// Without it lookups just go without.
fn init_overlay(path: &Path, init: fn(&str) -> Result<()>) -> Result<()> {
    match std::fs::read_to_string(path) {
        Ok(tsv) => init(&tsv).with_context(|| format!("init {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn init_examples(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    examples_core::init_from_bytes(&bytes).context("init examples")
}
