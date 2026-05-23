use serde::Deserialize;
use serde::Serialize;

use examples_types::ExampleEntry;
use jmdict_types::WordEntry;
use kanjidic_types::KanjiEntry;
use yomeru_shared::platform::{BoxFuture, DictClient};

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

impl DictClient for ExtensionDict {
    fn lookup<'a>(&'a self, word: &'a str) -> BoxFuture<'a, Result<Vec<WordEntry>, String>> {
        Box::pin(async move {
            let r: WordEntriesResp =
                crate::send_bg_message("LOOKUP_WORD", WordPayload { word }).await?;
            Ok(r.entries)
        })
    }

    fn lookup_many<'a>(
        &'a self,
        words: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<WordEntry>>, String>> {
        Box::pin(async move {
            let r: LookupManyResp =
                crate::send_bg_message("LOOKUP_MANY", LookupManyPayload { words }).await?;
            Ok(r.results)
        })
    }

    fn lookup_prefix<'a>(
        &'a self,
        text: &'a str,
        max: u8,
    ) -> BoxFuture<'a, Result<Vec<WordEntry>, String>> {
        Box::pin(async move {
            let r: LookupPrefixResp =
                crate::send_bg_message("LOOKUP_PREFIX", LookupPrefixPayload { text, max }).await?;
            Ok(r.results)
        })
    }

    fn kanji_for<'a>(&'a self, word: &'a str) -> BoxFuture<'a, Result<Vec<KanjiEntry>, String>> {
        Box::pin(async move {
            let r: KanjiResp =
                crate::send_bg_message("GET_KANJI", WordPayload { word }).await?;
            Ok(r.entries)
        })
    }

    fn examples_for<'a>(
        &'a self,
        word: &'a str,
        max: u8,
    ) -> BoxFuture<'a, Result<Vec<ExampleEntry>, String>> {
        Box::pin(async move {
            let r: ExamplesResp =
                crate::send_bg_message("GET_EXAMPLES", WordMaxPayload { word, max }).await?;
            Ok(r.entries)
        })
    }
}
