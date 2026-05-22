//! Pure-Rust examples (Tatoeba-style sentences) runtime. `examples-wasm` is
//! a thin `#[wasm_bindgen]` shim on top of this.

use anyhow::{anyhow, bail};
use examples_types::ExampleEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;

static EXAMPLES: OnceCell<ExamplesDictInner> = OnceCell::new();

struct ExamplesDictInner {
    index: Vec<(String, Vec<u32>)>,
    sentences_bytes: Vec<u8>,
}

/// Load the binary examples blob into the process-global cell.
pub fn init_from_bytes(bytes: &[u8]) -> anyhow::Result<()> {
    if EXAMPLES.get().is_some() {
        return Ok(());
    }
    let inner = parse_binary(bytes)?;
    EXAMPLES
        .set(inner)
        .map_err(|_| anyhow!("EXAMPLES already set"))?;
    Ok(())
}

pub fn is_loaded() -> bool {
    EXAMPLES.get().is_some()
}

/// Up to `max` example sentences for `headword`.
pub fn lookup(headword: &str, max: usize) -> Vec<ExampleEntry> {
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
