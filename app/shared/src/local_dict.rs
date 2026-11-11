//! The offline dictionary's files as Dioxus assets, for
//! [`yomeru_core::local_dict::LocalDict`].

use manganis::Asset;

pub use yomeru_core::local_dict::*;

/// The bundled data files, declared by the entry crate with `asset!` (see
/// `app/android/src/main.rs`). The overlays are optional, as on the server:
/// without them kanji cards just go without mnemonics / part names.
#[derive(Clone, Copy)]
pub struct DictAssets {
    pub jmdict: Asset,
    pub kanjidic: Asset,
    pub examples: Asset,
    pub mnemonics: Option<Asset>,
    pub part_names: Option<Asset>,
}

impl DictSource for DictAssets {
    fn read(&self, file: DictFile) -> Result<Option<Vec<u8>>, String> {
        let asset = match file {
            DictFile::Jmdict => Some(self.jmdict),
            DictFile::Kanjidic => Some(self.kanjidic),
            DictFile::Examples => Some(self.examples),
            DictFile::Mnemonics => self.mnemonics,
            DictFile::PartNames => self.part_names,
        };
        let Some(asset) = asset else {
            return Ok(None);
        };
        // Dioxus only offers an async reader. We're on a `spawn_blocking`
        // thread (see `DictSource::read`), which has the runtime's context
        // but isn't inside a task, so blocking on it here is allowed.
        let runtime = tokio::runtime::Handle::try_current().map_err(|e| e.to_string())?;
        runtime
            .block_on(dioxus::asset_resolver::read_asset_bytes(asset))
            .map(Some)
            .map_err(|e| format!("read {asset}: {e}"))
    }
}
