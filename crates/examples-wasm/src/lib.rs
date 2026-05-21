use examples_types::ExampleEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;
use wasm_bindgen::prelude::*;

static EXAMPLES: OnceCell<ExamplesDictInner> = OnceCell::new();

struct ExamplesDictInner {
    index: Vec<(String, Vec<u32>)>,
    sentences_bytes: Vec<u8>,
}

#[wasm_bindgen(start)]
pub fn init_examples_wasm() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct ExamplesDict {}

#[wasm_bindgen]
impl ExamplesDict {
    /// The examples blob is stored in a process-global `OnceCell`. A second
    /// call with different bytes silently reuses the first set — see the
    /// matching note on `jmdict-wasm::Dictionary::new`.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<ExamplesDict, JsError> {
        if EXAMPLES.get().is_some() {
            return Ok(ExamplesDict {});
        }
        let inner = parse_binary(bytes)
            .map_err(|e| JsError::new(&format!("Failed to load examples: {e}")))?;
        EXAMPLES
            .set(inner)
            .map_err(|_| JsError::new("ExamplesDict already initialized"))?;
        Ok(ExamplesDict {})
    }

    /// Returns up to `max` example sentences for `headword` as `{japanese, english}[]`.
    pub fn lookup(&self, headword: &str, max: u8) -> Result<JsValue, JsError> {
        let entries = lookup_entries(headword, max as usize);
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsError::new(&e.to_string()))
    }
}

/// Initialize the examples blob from raw bytes without going through wasm-bindgen.
pub fn init_from_bytes(bytes: &[u8]) -> anyhow::Result<()> {
    if EXAMPLES.get().is_some() {
        return Ok(());
    }
    let inner = parse_binary(bytes)?;
    EXAMPLES
        .set(inner)
        .map_err(|_| anyhow::anyhow!("EXAMPLES already set"))?;
    Ok(())
}

/// Pure-Rust: up to `max` example sentences for `headword`.
pub fn lookup(headword: &str, max: usize) -> Vec<ExampleEntry> {
    lookup_entries(headword, max)
}

fn lookup_entries(headword: &str, max: usize) -> Vec<ExampleEntry> {
    let dict = match EXAMPLES.get() {
        Some(d) => d,
        None => return vec![],
    };
    match dict.index.binary_search_by(|(k, _)| k.as_str().cmp(headword)) {
        Err(_) => vec![],
        Ok(i) => dict.index[i]
            .1
            .iter()
            .take(max)
            .filter_map(|&off| get_sentence(&dict.sentences_bytes, off))
            .collect(),
    }
}

fn get_sentence(bytes: &[u8], offset: u32) -> Option<ExampleEntry> {
    let pos = offset as usize;
    if pos + 4 > bytes.len() {
        return None;
    }
    let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
    let start = pos + 4;
    if start + len > bytes.len() {
        return None;
    }
    from_bytes(&bytes[start..start + len]).ok()
}

fn parse_binary(bytes: &[u8]) -> anyhow::Result<ExamplesDictInner> {
    use anyhow::bail;

    if bytes.len() < 9 {
        bail!("examples binary too short");
    }
    if &bytes[0..4] != b"EXPL" {
        bail!("invalid magic bytes");
    }
    if bytes[4] != 1 {
        bail!("unsupported version {}", bytes[4]);
    }

    let mut pos = 5usize;

    let read_u32 = |bytes: &[u8], pos: usize| -> anyhow::Result<usize> {
        if pos + 4 > bytes.len() {
            bail!("examples binary truncated reading length at {pos}");
        }
        Ok(u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize)
    };
    let read_slice = |bytes: &[u8], pos: usize, len: usize, what: &str| -> anyhow::Result<()> {
        if pos + len > bytes.len() {
            bail!("examples binary truncated reading {what} ({len} bytes at {pos})");
        }
        Ok(())
    };

    let index_len = read_u32(bytes, pos)?;
    pos += 4;
    read_slice(bytes, pos, index_len, "index")?;
    let index: Vec<(String, Vec<u32>)> = from_bytes(&bytes[pos..pos + index_len])?;
    pos += index_len;

    let sentences_len = read_u32(bytes, pos)?;
    pos += 4;
    read_slice(bytes, pos, sentences_len, "sentences")?;
    let sentences_bytes = bytes[pos..pos + sentences_len].to_vec();

    Ok(ExamplesDictInner {
        index,
        sentences_bytes,
    })
}
