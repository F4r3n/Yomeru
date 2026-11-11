//! The offline dictionary's files as Dioxus assets, for
//! [`yomeru_core::local_dict::LocalDict`].

use async_trait::async_trait;
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

#[async_trait(?Send)]
impl DictSource for DictAssets {
    async fn read(&self, file: DictFile) -> Result<Option<Vec<u8>>, String> {
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
        dioxus::asset_resolver::read_asset_bytes(asset)
            .await
            .map(Some)
            .map_err(|e| format!("read {asset}: {e}"))
    }
}
