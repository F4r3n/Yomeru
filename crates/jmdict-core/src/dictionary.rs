use anyhow::{anyhow, bail};
use fst::Map;
use jmdict_types::WordEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;

static DICT: OnceCell<DictionaryInner> = OnceCell::new();

pub(crate) struct DictionaryInner {
    pub(crate) fst: Map<Vec<u8>>,
    pub(crate) lookup_table: Vec<Vec<u32>>,
    pub(crate) entries_bytes: Vec<u8>,
}

/// Load the binary dictionary produced by `jmdict-build` into the process-global
/// `OnceCell`. Idempotent: a second call with different bytes silently reuses the
/// first set — if you ever need a reload API, migrate `DICT` to `arc-swap` or
/// `Mutex<Option<…>>`.
pub fn init(bytes: &[u8]) -> anyhow::Result<()> {
    if DICT.get().is_some() {
        return Ok(());
    }
    let inner = parse_binary(bytes)?;
    DICT.set(inner).map_err(|_| anyhow!("DICT already set"))?;
    Ok(())
}

/// Back-compat alias used by some host tests.
pub fn init_for_testing(bytes: &[u8]) -> anyhow::Result<()> {
    init(bytes)
}

pub(crate) fn fst_get(key: &str) -> Option<u64> {
    DICT.get()?.fst.get(key.as_bytes())
}

pub(crate) fn get_entry_group(group_idx: u64) -> Option<Vec<u32>> {
    DICT.get()
        .and_then(|d| d.lookup_table.get(group_idx as usize).cloned())
}

pub(crate) fn get_entry(idx: u32) -> Option<WordEntry> {
    let dict = DICT.get()?;
    let bytes = &dict.entries_bytes;
    let pos = idx as usize;
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

pub(crate) fn fst_prefix_search(prefix: &str) -> Vec<(String, u64)> {
    let dict = match DICT.get() {
        Some(d) => d,
        None => return vec![],
    };
    use fst::Automaton;
    use fst::automaton::Str;
    let automaton = Str::new(prefix).starts_with();
    use fst::IntoStreamer;
    use fst::Streamer;
    let mut stream = dict.fst.search(automaton).into_stream();
    let mut results = Vec::new();
    while let Some((k, v)) = stream.next() {
        if let Ok(s) = std::str::from_utf8(k) {
            results.push((s.to_owned(), v));
        }
    }
    results
}

pub fn is_loaded() -> bool {
    DICT.get().is_some()
}

fn parse_binary(bytes: &[u8]) -> anyhow::Result<DictionaryInner> {
    if bytes.len() < 9 {
        bail!("Dictionary binary too short");
    }
    if &bytes[0..4] != b"JMDI" {
        bail!("Invalid magic bytes");
    }
    if bytes[4] != 1 {
        bail!("Unsupported dictionary version {}", bytes[4]);
    }

    let mut pos = 5usize;

    let read_u32 = |bytes: &[u8], pos: usize| -> anyhow::Result<usize> {
        if pos + 4 > bytes.len() {
            bail!("dictionary binary truncated reading length at {pos}");
        }
        Ok(u32::from_le_bytes(bytes[pos..pos + 4].try_into()?) as usize)
    };
    let read_slice = |bytes: &[u8], pos: usize, len: usize, what: &str| -> anyhow::Result<()> {
        if pos + len > bytes.len() {
            bail!("dictionary binary truncated reading {what} ({len} bytes at {pos})");
        }
        Ok(())
    };

    let fst_len = read_u32(bytes, pos)?;
    pos += 4;
    read_slice(bytes, pos, fst_len, "fst")?;
    let fst_bytes = bytes[pos..pos + fst_len].to_vec();
    pos += fst_len;

    let lt_len = read_u32(bytes, pos)?;
    pos += 4;
    read_slice(bytes, pos, lt_len, "lookup table")?;
    let lookup_table: Vec<Vec<u32>> = from_bytes(&bytes[pos..pos + lt_len])?;
    pos += lt_len;

    let entries_len = read_u32(bytes, pos)?;
    pos += 4;
    read_slice(bytes, pos, entries_len, "entries")?;
    let entries_bytes = bytes[pos..pos + entries_len].to_vec();

    let fst = Map::new(fst_bytes)?;

    Ok(DictionaryInner {
        fst,
        lookup_table,
        entries_bytes,
    })
}
