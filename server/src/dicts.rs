//! Loads jmdict.bin / kanjidic.bin / examples.bin / grammar.bin at startup.
//! The underlying crates store their parsed state in process-global
//! `OnceCell`s (grammar-core has none: [`grammar`] holds it), so once
//! `init_all` returns successfully the lookup fns can be called from any
//! handler without further plumbing.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use grammar_core::GrammarDict;

use anyhow::{Context, Result};

pub fn init_all(data_dir: &str) -> Result<()> {
    let dir = PathBuf::from(data_dir);
    init_jmdict(&dir.join("jmdict.bin"))?;
    init_kanjidic(&dir.join("kanjidic.bin"))?;
    init_overlay(
        &dir.join("kanji-mnemonics.tsv"),
        kanjidic_core::init_mnemonics,
    )?;
    init_overlay(&dir.join("part-names.tsv"), kanjidic_core::init_part_names)?;
    init_examples(&dir.join("examples.bin"))?;
    init_grammar(&dir.join("grammar.bin"))?;
    Ok(())
}

static GRAMMAR: OnceLock<GrammarDict> = OnceLock::new();

/// The grammar dictionary, once [`init_all`] (or a test) loaded it.
pub fn grammar() -> Option<&'static GrammarDict> {
    GRAMMAR.get()
}

/// Loads `grammar.bin` bytes; a second call keeps the first dictionary.
pub fn init_grammar_from_bytes(bytes: &[u8]) -> Result<()> {
    let dict = GrammarDict::from_bytes(bytes).context("init grammar")?;
    let _ = GRAMMAR.set(dict);
    Ok(())
}

fn init_grammar(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    init_grammar_from_bytes(&bytes)
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
