use serde::Deserialize;
use serde::Serialize;

use async_trait::async_trait;
use examples_types::ExampleEntry;
use jmdict_types::WordEntry;
use kanjidic_types::KanjiEntry;
use yomeru_shared::platform::DictClient;
pub struct ExtensionDict;

#[derive(Serialize)]
struct WordPayload<'a> {
    word: &'a str,
}

#[derive(Serialize)]
struct WordMaxPayload<'a> {
    word: &'a str,
    max: u8,
}

#[derive(Serialize)]
struct LookupManyPayload<'a> {
    words: &'a [String],
}

#[derive(Serialize)]
struct LookupPrefixPayload<'a> {
    text: &'a str,
    max: u8,
}

#[derive(Serialize)]
struct LookupBySequencePayload<'a> {
    sequences: &'a [u32],
}

#[derive(Deserialize)]
struct LookupBySequenceResp {
    results: Vec<Option<WordEntry>>,
}

#[derive(Deserialize)]
struct WordEntriesResp {
    entries: Vec<WordEntry>,
}

#[derive(Deserialize)]
struct LookupManyResp {
    results: Vec<Vec<WordEntry>>,
}

#[derive(Deserialize)]
struct LookupPrefixResp {
    results: Vec<WordEntry>,
}

#[derive(Deserialize)]
struct KanjiResp {
    entries: Vec<KanjiEntry>,
}

#[derive(Deserialize)]
struct ExamplesResp {
    entries: Vec<ExampleEntry>,
}

#[async_trait(?Send)]
impl DictClient for ExtensionDict {
    async fn lookup(&self, word: &str) -> Result<Vec<WordEntry>, String> {
        let r: WordEntriesResp =
            crate::send_bg_message("LOOKUP_WORD", WordPayload { word }).await?;
        Ok(r.entries)
    }

    async fn lookup_many(&self, words: &[String]) -> Result<Vec<Vec<WordEntry>>, String> {
        let r: LookupManyResp =
            crate::send_bg_message("LOOKUP_MANY", LookupManyPayload { words }).await?;
        Ok(r.results)
    }

    async fn lookup_by_sequence(&self, sequences: &[u32]) -> Result<Vec<Option<WordEntry>>, String> {
        let r: LookupBySequenceResp = crate::send_bg_message(
            "LOOKUP_BY_SEQUENCE",
            LookupBySequencePayload { sequences },
        )
        .await?;
        Ok(r.results)
    }

    async fn lookup_prefix(&self, text: &str, max: u8) -> Result<Vec<WordEntry>, String> {
        let r: LookupPrefixResp =
            crate::send_bg_message("LOOKUP_PREFIX", LookupPrefixPayload { text, max }).await?;
        Ok(r.results)
    }

    async fn kanji_for(&self, word: &str) -> Result<Vec<KanjiEntry>, String> {
        let r: KanjiResp = crate::send_bg_message("GET_KANJI", WordPayload { word }).await?;
        Ok(r.entries)
    }

    async fn examples_for(&self, word: &str, max: u8) -> Result<Vec<ExampleEntry>, String> {
        let r: ExamplesResp =
            crate::send_bg_message("GET_EXAMPLES", WordMaxPayload { word, max }).await?;
        Ok(r.entries)
    }
}
