use anyhow::{anyhow, bail};
use fst::Map;
use jmdict_types::ArchivedWordEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;

static DICT: OnceCell<DictionaryInner> = OnceCell::new();

pub(crate) struct DictionaryInner {
    pub(crate) fst: Map<Vec<u8>>,
    pub(crate) lookup_table: Vec<Vec<u32>>,
    pub(crate) entries_bytes: Vec<u8>,
    /// (ent_seq, byte_offset) sorted by ent_seq, for binary-search lookup.
    pub(crate) seq_index: Vec<(u32, u32)>,
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

/// Zero-copy access to the entry at byte `idx` in the entries blob.
///
/// Returns a reference straight into the process-global buffer — no allocation,
/// no decode. The `'static` lifetime is sound because `DICT` is a `OnceCell`
/// that, once set, lives for the rest of the process and is never mutated.
pub(crate) fn get_entry(idx: u32) -> Option<&'static ArchivedWordEntry> {
    let dict = DICT.get()?;
    let bytes: &'static [u8] = dict.entries_bytes.as_slice();
    let pos = idx as usize;
    let len_end = pos.checked_add(4)?;
    let len_bytes = bytes.get(pos..len_end)?;
    let len = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
    let end = len_end.checked_add(len)?;
    let entry_bytes = bytes.get(len_end..end)?;
    // SAFETY: every entry in the blob was bytecheck-validated once at init
    // (see `validate_entries`), and the buffer is immutable for 'static, so the
    // archived layout is known-good and outlives this reference.
    Some(unsafe { rkyv::access_unchecked::<ArchivedWordEntry>(entry_bytes) })
}

/// Look up an entry by its JMdict ent_seq (sequence) number.
pub fn lookup_by_sequence(seq: u32) -> Option<&'static ArchivedWordEntry> {
    let dict = DICT.get()?;
    let pos = dict
        .seq_index
        .binary_search_by_key(&seq, |(s, _)| *s)
        .ok()?;
    let offset = dict.seq_index.get(pos)?.1;
    get_entry(offset)
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
    if bytes[4] != 4 {
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
    pos += entries_len;

    let seq_len = read_u32(bytes, pos)?;
    pos += 4;
    read_slice(bytes, pos, seq_len, "seq index")?;
    let seq_index: Vec<(u32, u32)> = from_bytes(&bytes[pos..pos + seq_len])?;

    // Validate every archived entry once, up front. This turns a corrupt blob —
    // or a `jmdict-types/full` feature mismatch between builder and reader — into
    // a loud init error instead of silent garbage, and lets `get_entry` use the
    // unchecked (zero-cost) access on the hot path.
    validate_entries(&entries_bytes)?;

    let fst = Map::new(fst_bytes)?;

    Ok(DictionaryInner {
        fst,
        lookup_table,
        entries_bytes,
        seq_index,
    })
}

/// Walk the length-prefixed entries blob and bytecheck-validate each archived
/// `WordEntry`. Run once at init so the runtime hot path can skip validation.
fn validate_entries(bytes: &[u8]) -> anyhow::Result<()> {
    let mut pos = 0usize;
    while pos < bytes.len() {
        let len_end = pos
            .checked_add(4)
            .ok_or_else(|| anyhow!("entry length overflow at {pos}"))?;
        let len_bytes = bytes
            .get(pos..len_end)
            .ok_or_else(|| anyhow!("truncated entry length at {pos}"))?;
        let len = u32::from_le_bytes(len_bytes.try_into()?) as usize;
        let end = len_end
            .checked_add(len)
            .ok_or_else(|| anyhow!("entry body overflow at {pos}"))?;
        let entry = bytes
            .get(len_end..end)
            .ok_or_else(|| anyhow!("truncated entry body at {pos}"))?;
        rkyv::access::<ArchivedWordEntry, rkyv::rancor::Error>(entry)
            .map_err(|e| anyhow!("entry at offset {pos} failed rkyv validation: {e}"))?;
        pos = end;
    }
    Ok(())
}
