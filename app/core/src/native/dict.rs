//! Offline dictionary: the same lookups the server's `/api/*` endpoints
//! answer, run in-process against `jmdict.bin`, `kanjidic.bin` and
//! `examples.bin` bundled with the app.
//!
//! The data ships with the app, read through a [`DictSource`](crate::local_dict::DictSource) the UI
//! provides (Dioxus assets: `yomeru_shared::local_dict::DictAssets`, declared
//! in `app/android/src/main.rs`), so only builds that ask for it carry the
//! ~70 MB. Loading is lazy and happens once: the first lookup — or
//! [`DictClient::warm_up`](crate::platform::DictClient::warm_up) at startup — reads the assets and parses them on
//! a blocking thread; every later call finds the cores ready.

use examples_types::ExampleEntry;
use jmdict_types::{ArchivedWordEntry, WordEntry};
use kanjidic_types::{KanjiEntry, PartInfo, PartSearch};
use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::platform::DictClient;
use async_trait::async_trait;

/// One of the bundled data files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictFile {
    Jmdict,
    Kanjidic,
    Examples,
    /// Optional overlay, as on the server: without it kanji cards just go
    /// without mnemonics.
    Mnemonics,
    /// Optional overlay: without it the kanji finder shows bare parts.
    PartNames,
}

/// Where the bundled files come from.
pub trait DictSource: Send + Sync {
    /// The file's bytes, or `Ok(None)` when this build doesn't bundle it.
    /// Called on a blocking thread (tokio's `spawn_blocking` pool), so it
    /// may block on I/O.
    fn read(&self, file: DictFile) -> Result<Option<Vec<u8>>, String>;
}

pub struct LocalDict {
    source: Arc<dyn DictSource>,
}

impl LocalDict {
    pub fn new(source: Arc<dyn DictSource>) -> Self {
        Self { source }
    }

    async fn ensure(&self) -> Result<(), String> {
        ensure(self.source.clone()).await
    }
}

/// Process-global like the cores it guards: they live in `OnceCell`s of
/// their own and can only be initialized once anyway.
static READY: OnceCell<Result<(), String>> = OnceCell::const_new();

/// Loads every core once; concurrent callers wait on the same load. A
/// failure is remembered rather than retried: the assets are part of the app,
/// so a second attempt would fail the same way and pay the read again.
async fn ensure(source: Arc<dyn DictSource>) -> Result<(), String> {
    READY.get_or_init(|| load(source)).await.clone()
}

fn read(source: &dyn DictSource, file: DictFile) -> Result<Vec<u8>, String> {
    source
        .read(file)?
        .ok_or_else(|| format!("{file:?} isn't bundled"))
}

fn read_text(source: &dyn DictSource, file: DictFile) -> Result<Option<String>, String> {
    let Some(bytes) = source.read(file)? else {
        return Ok(None);
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| format!("{file:?}: {e}"))
}

async fn load(source: Arc<dyn DictSource>) -> Result<(), String> {
    let started = crate::rt::now_ms();
    // Reading ~70 MB, then validating every entry (bytecheck) and building the
    // indexes: a second or so of I/O and CPU on a phone, kept off the UI
    // thread.
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let source = &*source;
        let jmdict = read(source, DictFile::Jmdict)?;
        let kanjidic = read(source, DictFile::Kanjidic)?;
        let examples = read(source, DictFile::Examples)?;
        let mnemonics = read_text(source, DictFile::Mnemonics)?;
        let part_names = read_text(source, DictFile::PartNames)?;
        init_cores(&jmdict, &kanjidic, &examples, mnemonics, part_names)
            .map_err(|e| format!("load dictionary: {e:#}"))
    })
    .await
    .map_err(|e| format!("dictionary loader: {e}"))??;
    tracing::info!(
        "[yomeru] dictionaries loaded in {:.0} ms",
        crate::rt::now_ms() - started
    );
    Ok(())
}

fn init_cores(
    jmdict: &[u8],
    kanjidic: &[u8],
    examples: &[u8],
    mnemonics: Option<String>,
    part_names: Option<String>,
) -> anyhow::Result<()> {
    jmdict_core::init(jmdict)?;
    kanjidic_core::init_from_bytes(kanjidic)?;
    if let Some(tsv) = &mnemonics {
        kanjidic_core::init_mnemonics(tsv)?;
    }
    if let Some(tsv) = &part_names {
        kanjidic_core::init_part_names(tsv)?;
    }
    examples_core::init_from_bytes(examples)?;
    Ok(())
}

/// The owned entry the UI works with, decoded straight from the archived
/// buffer. Same data the server's JSON carries, without the round trip.
fn owned(e: &ArchivedWordEntry) -> Option<WordEntry> {
    rkyv::deserialize::<WordEntry, rkyv::rancor::Error>(e)
        .map_err(|err| tracing::warn!("[yomeru] decoding entry: {err}"))
        .ok()
}

fn owned_all(entries: Vec<&ArchivedWordEntry>) -> Vec<WordEntry> {
    entries.into_iter().filter_map(owned).collect()
}

#[async_trait]
impl DictClient for LocalDict {
    fn warm_up(&self) {
        let source = self.source.clone();
        crate::rt::spawn_detached(async move {
            if let Err(e) = ensure(source).await {
                tracing::error!("[yomeru] {e}");
            }
        });
    }

    async fn lookup(&self, word: &str) -> Result<Vec<WordEntry>, String> {
        self.ensure().await?;
        Ok(owned_all(jmdict_core::lookup(word)))
    }

    async fn lookup_many(&self, words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
        self.ensure().await?;
        Ok(words
            .iter()
            .map(|w| owned_all(jmdict_core::lookup(w)))
            .collect())
    }

    async fn lookup_by_sequence(
        &self,
        sequences: &[u32],
    ) -> Result<Vec<Option<WordEntry>>, String> {
        self.ensure().await?;
        Ok(sequences
            .iter()
            .map(|s| jmdict_core::lookup_by_sequence(*s).and_then(owned))
            .collect())
    }

    async fn lookup_prefix(&self, text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
        self.ensure().await?;
        Ok(owned_all(jmdict_core::lookup_prefix(text, max)))
    }

    async fn kanji_for(&self, word: &str) -> Result<Vec<KanjiEntry>, String> {
        self.ensure().await?;
        Ok(kanjidic_core::lookup_many(word))
    }

    async fn examples_for(&self, word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
        self.ensure().await?;
        Ok(examples_core::lookup(word, usize::from(max)))
    }

    async fn kanji_parts(&self) -> Result<Vec<PartInfo>, String> {
        self.ensure().await?;
        Ok(kanjidic_core::parts())
    }

    async fn kanji_with_parts(&self, parts: &str, max: u8) -> Result<PartSearch, String> {
        self.ensure().await?;
        let parts: Vec<char> = parts.chars().collect();
        Ok(kanjidic_core::kanji_with_parts(&parts, usize::from(max)))
    }
}
