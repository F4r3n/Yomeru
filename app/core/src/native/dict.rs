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

    /// Waits until `part` is loaded, loading it now if nothing has started it.
    async fn ensure(&self, part: Part) -> Result<(), String> {
        ensure(part, self.source.clone()).await
    }
}

/// The three dictionaries, loaded independently so a screen only waits for
/// the one it reads: Review needs the words at once, the example sentences
/// can arrive later.
#[derive(Clone, Copy, Debug)]
enum Part {
    /// JMdict: headwords, readings, glosses.
    Words,
    /// KANJIDIC, plus the mnemonic and part-name overlays.
    Kanji,
    /// Example sentences.
    Examples,
}

/// Process-global like the cores they guard: those live in `OnceCell`s of
/// their own and can only be initialized once anyway.
static WORDS: OnceCell<Result<(), String>> = OnceCell::const_new();
static KANJI: OnceCell<Result<(), String>> = OnceCell::const_new();
static EXAMPLES: OnceCell<Result<(), String>> = OnceCell::const_new();

impl Part {
    fn ready(self) -> &'static OnceCell<Result<(), String>> {
        match self {
            Self::Words => &WORDS,
            Self::Kanji => &KANJI,
            Self::Examples => &EXAMPLES,
        }
    }

    /// Reads this part's files and initializes its cores. Blocking: run on
    /// a `spawn_blocking` thread.
    fn load_blocking(self, source: &dyn DictSource, steps: &mut Steps) -> Result<(), String> {
        let init_err = |e: anyhow::Error| format!("load {self:?} dictionary: {e:#}");
        match self {
            Self::Words => {
                let bytes = steps.time("read", || read(source, DictFile::Jmdict))?;
                steps
                    .time("init", || jmdict_core::init(&bytes))
                    .map_err(init_err)
            }
            Self::Kanji => {
                let bytes = steps.time("read", || read(source, DictFile::Kanjidic))?;
                let mnemonics =
                    steps.time("read mnemonics", || read_text(source, DictFile::Mnemonics))?;
                let part_names =
                    steps.time("read part names", || read_text(source, DictFile::PartNames))?;
                steps
                    .time("init", || -> anyhow::Result<()> {
                        kanjidic_core::init_from_bytes(&bytes)?;
                        if let Some(tsv) = &mnemonics {
                            kanjidic_core::init_mnemonics(tsv)?;
                        }
                        if let Some(tsv) = &part_names {
                            kanjidic_core::init_part_names(tsv)?;
                        }
                        Ok(())
                    })
                    .map_err(init_err)
            }
            Self::Examples => {
                let bytes = steps.time("read", || read(source, DictFile::Examples))?;
                steps
                    .time("init", || examples_core::init_from_bytes(&bytes))
                    .map_err(init_err)
            }
        }
    }
}

/// Loads `part` once; concurrent callers wait on the same load. A failure is
/// remembered rather than retried: the assets are part of the app, so a
/// second attempt would fail the same way and pay the read again.
async fn ensure(part: Part, source: Arc<dyn DictSource>) -> Result<(), String> {
    part.ready()
        .get_or_init(|| load(part, source))
        .await
        .clone()
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

async fn load(part: Part, source: Arc<dyn DictSource>) -> Result<(), String> {
    let started = crate::rt::now_ms();
    // Reading the file out of the APK (which decompresses it), then
    // validating every entry (bytecheck) and building the indexes: up to a
    // second or so on a phone for the words, kept off the UI thread.
    let steps = tokio::task::spawn_blocking(move || -> Result<Steps, String> {
        let mut steps = Steps::default();
        part.load_blocking(&*source, &mut steps)?;
        Ok(steps)
    })
    .await
    .map_err(|e| format!("dictionary loader: {e}"))??;
    tracing::info!(
        "[yomeru] {part:?} dictionary loaded in {:.0} ms ({})",
        crate::rt::now_ms() - started,
        steps.summary()
    );
    Ok(())
}

/// How long each loading step took, for the "dictionary loaded" log line.
#[derive(Default)]
struct Steps(Vec<(&'static str, f64)>);

impl Steps {
    fn time<T>(&mut self, name: &'static str, step: impl FnOnce() -> T) -> T {
        let started = crate::rt::now_ms();
        let out = step();
        self.0.push((name, crate::rt::now_ms() - started));
        out
    }

    fn summary(&self) -> String {
        self.0
            .iter()
            .map(|(name, ms)| format!("{name} {ms:.0}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
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
        // One after the other, words first: they're what the start screen
        // (Review) waits for, so they get the disk and CPU to themselves.
        let source = self.source.clone();
        crate::rt::spawn_detached(async move {
            for part in [Part::Words, Part::Kanji, Part::Examples] {
                if let Err(e) = ensure(part, source.clone()).await {
                    tracing::error!("[yomeru] {e}");
                }
            }
        });
    }

    async fn lookup(&self, word: &str) -> Result<Vec<WordEntry>, String> {
        self.ensure(Part::Words).await?;
        Ok(owned_all(jmdict_core::lookup(word)))
    }

    async fn lookup_many(&self, words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
        self.ensure(Part::Words).await?;
        Ok(words
            .iter()
            .map(|w| owned_all(jmdict_core::lookup(w)))
            .collect())
    }

    async fn lookup_by_sequence(
        &self,
        sequences: &[u32],
    ) -> Result<Vec<Option<WordEntry>>, String> {
        self.ensure(Part::Words).await?;
        Ok(sequences
            .iter()
            .map(|s| jmdict_core::lookup_by_sequence(*s).and_then(owned))
            .collect())
    }

    async fn lookup_prefix(&self, text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
        self.ensure(Part::Words).await?;
        Ok(owned_all(jmdict_core::lookup_prefix(text, max)))
    }

    async fn kanji_for(&self, word: &str) -> Result<Vec<KanjiEntry>, String> {
        self.ensure(Part::Kanji).await?;
        Ok(kanjidic_core::lookup_many(word))
    }

    async fn examples_for(&self, word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
        self.ensure(Part::Examples).await?;
        Ok(examples_core::lookup(word, usize::from(max)))
    }

    async fn kanji_parts(&self) -> Result<Vec<PartInfo>, String> {
        self.ensure(Part::Kanji).await?;
        Ok(kanjidic_core::parts())
    }

    async fn kanji_with_parts(&self, parts: &str, max: u8) -> Result<PartSearch, String> {
        self.ensure(Part::Kanji).await?;
        let parts: Vec<char> = parts.chars().collect();
        Ok(kanjidic_core::kanji_with_parts(&parts, usize::from(max)))
    }
}
