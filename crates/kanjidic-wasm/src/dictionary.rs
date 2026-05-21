use anyhow::bail;
use japanese_utils::is_kanji;
use kanjidic_types::KanjiEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;
use wasm_bindgen::prelude::*;

static KDICT: OnceCell<KanjiDictInner> = OnceCell::new();

struct KanjiDictInner {
    index: Vec<(u32, u32)>,
    data: Vec<u8>,
}

#[wasm_bindgen]
pub struct KanjiDictionary {}

#[wasm_bindgen]
impl KanjiDictionary {
    /// The kanjidic is stored in a process-global `OnceCell`. A second call
    /// with different bytes silently reuses the first set — see the matching
    /// note on `jmdict-wasm::Dictionary::new`.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<KanjiDictionary, JsError> {
        if KDICT.get().is_some() {
            return Ok(KanjiDictionary {});
        }
        let inner = parse_binary(bytes)
            .map_err(|e| JsError::new(&format!("Failed to load kanjidic: {e}")))?;
        KDICT
            .set(inner)
            .map_err(|_| JsError::new("KanjiDictionary already initialized"))?;
        Ok(KanjiDictionary {})
    }

    /// Single-char string → KanjiEntry | null
    pub fn lookup(&self, ch: &str) -> Result<JsValue, JsError> {
        let c = ch
            .chars()
            .next()
            .ok_or_else(|| JsError::new("empty string"))?;
        match get_entry(c as u32) {
            Some(e) => serde_wasm_bindgen::to_value(&e)
                .map_err(|e| JsError::new(&e.to_string())),
            None => Ok(JsValue::null()),
        }
    }

    /// Extract kanji chars from word, return array of KanjiEntry for each found.
    pub fn lookup_many(&self, word: &str) -> Result<JsValue, JsError> {
        let entries: Vec<KanjiEntry> = word
            .chars()
            .filter(|&c| is_kanji(c))
            .filter_map(|c| get_entry(c as u32))
            .collect();
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))
    }
}

/// Initialize the kanjidic from raw bytes without going through wasm-bindgen.
pub fn init_from_bytes(bytes: &[u8]) -> anyhow::Result<()> {
    if KDICT.get().is_some() {
        return Ok(());
    }
    let inner = parse_binary(bytes)?;
    KDICT
        .set(inner)
        .map_err(|_| anyhow::anyhow!("KDICT already set"))?;
    Ok(())
}

/// Pure-Rust lookup of one kanji character.
pub fn lookup_one(ch: char) -> Option<KanjiEntry> {
    get_entry(ch as u32)
}

/// Pure-Rust: extract kanji chars from `word`, return entries for each.
pub fn lookup_many(word: &str) -> Vec<KanjiEntry> {
    word.chars()
        .filter(|&c| is_kanji(c))
        .filter_map(|c| get_entry(c as u32))
        .collect()
}

fn get_entry(codepoint: u32) -> Option<KanjiEntry> {
    let dict = KDICT.get()?;
    let pos = dict
        .index
        .binary_search_by_key(&codepoint, |&(cp, _)| cp)
        .ok()?;
    let byte_offset = dict.index[pos].1 as usize;
    let bytes = &dict.data;
    if byte_offset + 4 > bytes.len() {
        return None;
    }
    let len =
        u32::from_le_bytes(bytes[byte_offset..byte_offset + 4].try_into().ok()?) as usize;
    let start = byte_offset + 4;
    if start + len > bytes.len() {
        return None;
    }
    from_bytes(&bytes[start..start + len]).ok()
}

fn parse_binary(bytes: &[u8]) -> anyhow::Result<KanjiDictInner> {
    if bytes.len() < 9 {
        bail!("kanjidic binary too short");
    }
    if &bytes[0..4] != b"KDIC" {
        bail!("invalid magic bytes");
    }
    if bytes[4] != 1 {
        bail!("unsupported version {}", bytes[4]);
    }

    let mut pos = 5usize;
    let count = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize;
    pos += 4;

    let mut index = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 8 > bytes.len() {
            bail!("index truncated");
        }
        let cp = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?);
        let off = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into()?);
        index.push((cp, off));
        pos += 8;
    }

    if pos + 4 > bytes.len() {
        bail!("data_len field missing");
    }
    let data_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize;
    pos += 4;

    if pos + data_len > bytes.len() {
        bail!("data blob truncated");
    }
    let data = bytes[pos..pos + data_len].to_vec();

    Ok(KanjiDictInner { index, data })
}
