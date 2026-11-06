//! Offline dictionary: the same lookups the server's `/api/*` endpoints
//! answer, run in-process against `jmdict.bin`, `kanjidic.bin` and
//! `examples.bin` bundled with the app.
//!
//! The data ships as assets declared by the entry crate (see
//! `app/android/src/main.rs`), so only builds that ask for it carry the
//! ~70 MB. Loading is lazy and happens once: the first lookup — or
//! [`DictClient::warm_up`] at startup — reads the assets and parses them on
//! a blocking thread; every later call finds the cores ready.

use examples_types::ExampleEntry;
use jmdict_types::{ArchivedWordEntry, WordEntry};
use kanjidic_types::KanjiEntry;
use manganis::Asset;
use tokio::sync::OnceCell;

use crate::platform::DictClient;
use async_trait::async_trait;

/// The bundled data files. The overlays are optional, as on the server:
/// without them kanji cards just go without mnemonics / part names.
#[derive(Clone, Copy)]
pub struct DictAssets {
    pub jmdict: Asset,
    pub kanjidic: Asset,
    pub examples: Asset,
    pub mnemonics: Option<Asset>,
    pub part_names: Option<Asset>,
}

pub struct LocalDict {
    assets: DictAssets,
}

impl LocalDict {
    pub fn new(assets: DictAssets) -> Self {
        Self { assets }
    }

    async fn ensure(&self) -> Result<(), String> {
        ensure(self.assets).await
    }
}

/// Process-global like the cores it guards: they live in `OnceCell`s of
/// their own and can only be initialized once anyway.
static READY: OnceCell<Result<(), String>> = OnceCell::const_new();

/// Loads every core once; concurrent callers wait on the same load. A
/// failure is remembered rather than retried: the assets are part of the app,
/// so a second attempt would fail the same way and pay the read again.
async fn ensure(assets: DictAssets) -> Result<(), String> {
    READY.get_or_init(|| load(assets)).await.clone()
}

async fn read(asset: Asset) -> Result<Vec<u8>, String> {
    dioxus::asset_resolver::read_asset_bytes(asset)
        .await
        .map_err(|e| format!("read {asset}: {e}"))
}

async fn read_text(asset: Option<Asset>) -> Result<Option<String>, String> {
    let Some(asset) = asset else {
        return Ok(None);
    };
    String::from_utf8(read(asset).await?)
        .map(Some)
        .map_err(|e| format!("{asset}: {e}"))
}

async fn load(assets: DictAssets) -> Result<(), String> {
    let started = crate::rt::now_ms();
    let jmdict = read(assets.jmdict).await?;
    let kanjidic = read(assets.kanjidic).await?;
    let examples = read(assets.examples).await?;
    let mnemonics = read_text(assets.mnemonics).await?;
    let part_names = read_text(assets.part_names).await?;
    // Parsing validates every entry (bytecheck) and builds the indexes: a
    // second or so of CPU on a phone, kept off the UI thread.
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        jmdict_core::init(&jmdict)?;
        kanjidic_core::init_from_bytes(&kanjidic)?;
        if let Some(tsv) = &mnemonics {
            kanjidic_core::init_mnemonics(tsv)?;
        }
        if let Some(tsv) = &part_names {
            kanjidic_core::init_part_names(tsv)?;
        }
        examples_core::init_from_bytes(&examples)?;
        Ok(())
    })
    .await
    .map_err(|e| format!("dictionary loader: {e}"))?
    .map_err(|e| format!("load dictionary: {e:#}"))?;
    dioxus::logger::tracing::info!(
        "[yomeru] dictionaries loaded in {:.0} ms",
        crate::rt::now_ms() - started
    );
    Ok(())
}

/// The owned entry the UI works with, decoded straight from the archived
/// buffer. Same data the server's JSON carries, without the round trip.
fn owned(e: &ArchivedWordEntry) -> Option<WordEntry> {
    rkyv::deserialize::<WordEntry, rkyv::rancor::Error>(e)
        .map_err(|err| dioxus::logger::tracing::warn!("[yomeru] decoding entry: {err}"))
        .ok()
}

fn owned_all(entries: Vec<&ArchivedWordEntry>) -> Vec<WordEntry> {
    entries.into_iter().filter_map(owned).collect()
}

#[async_trait(?Send)]
impl DictClient for LocalDict {
    fn warm_up(&self) {
        let assets = self.assets;
        crate::rt::spawn_detached(async move {
            if let Err(e) = ensure(assets).await {
                dioxus::logger::tracing::error!("[yomeru] {e}");
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
}
