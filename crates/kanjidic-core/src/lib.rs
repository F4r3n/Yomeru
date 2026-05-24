//! Pure-Rust KANJIDIC runtime: binary loader + lookup. `kanjidic-wasm` is a
//! thin `#[wasm_bindgen]` shim on top of this.

use anyhow::{anyhow, bail};
use japanese_utils::is_kanji;
use kanjidic_types::KanjiEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;

static KDICT: OnceCell<KanjiDictInner> = OnceCell::new();

struct KanjiDictInner {
    index: Vec<(u32, u32)>,
    data: Vec<u8>,
}

/// Load a KANJIDIC binary into the process-global cell. Idempotent: a second
/// call with different bytes silently reuses the first set.
pub fn init_from_bytes(bytes: &[u8]) -> anyhow::Result<()> {
    if KDICT.get().is_some() {
        return Ok(());
    }
    let inner = parse_binary(bytes)?;
    KDICT.set(inner).map_err(|_| anyhow!("KDICT already set"))?;
    Ok(())
}

pub fn is_loaded() -> bool {
    KDICT.get().is_some()
}

/// Lookup one kanji character.
pub fn lookup_one(ch: char) -> Option<KanjiEntry> {
    get_entry(ch as u32)
}

/// Extract kanji chars from `word`, return entries for each.
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

#[cfg(test)]
mod tests {
    use super::*;
    use kanjidic_types::KanjiEntry;
    use postcard::to_allocvec;

    fn entry(literal: char, strokes: u8, on: &[&str], kun: &[&str], meanings: &[&str]) -> KanjiEntry {
        KanjiEntry {
            literal,
            stroke_count: strokes,
            grade: None,
            freq: None,
            jlpt: None,
            on_readings: on.iter().map(|s| s.to_string()).collect(),
            kun_readings: kun.iter().map(|s| s.to_string()).collect(),
            meanings: meanings.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Serialize a small in-memory KDIC binary matching the format documented
    /// in `parse_binary`. The index is sorted by codepoint to match the real
    /// builder's invariant (`get_entry` does a binary search).
    fn build_binary(entries: &[KanjiEntry]) -> Vec<u8> {
        let mut sorted: Vec<&KanjiEntry> = entries.iter().collect();
        sorted.sort_by_key(|e| e.literal as u32);

        let mut out = Vec::new();
        out.extend_from_slice(b"KDIC");
        out.push(1u8);
        out.extend_from_slice(&(sorted.len() as u32).to_le_bytes());

        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(sorted.len());
        for e in &sorted {
            offsets.push(data.len() as u32);
            let ser = to_allocvec(*e).unwrap();
            data.extend_from_slice(&(ser.len() as u32).to_le_bytes());
            data.extend_from_slice(&ser);
        }
        for (e, off) in sorted.iter().zip(offsets.iter()) {
            out.extend_from_slice(&(e.literal as u32).to_le_bytes());
            out.extend_from_slice(&off.to_le_bytes());
        }
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn parse_binary_roundtrip() {
        let e1 = entry('漢', 13, &["カン"], &[], &["Sino-", "China"]);
        let e2 = entry('字', 6, &["ジ"], &["あざ"], &["character", "letter"]);
        let bin = build_binary(&[e1.clone(), e2.clone()]);

        let inner = parse_binary(&bin).expect("parse");
        assert_eq!(inner.index.len(), 2);
        // Sorted ascending by codepoint already (字 = 0x5B57 < 漢 = 0x6F22).
        assert_eq!(inner.index[0].0, '字' as u32);
        assert_eq!(inner.index[1].0, '漢' as u32);
    }

    #[test]
    fn parse_binary_rejects_short_input() {
        let err = parse_binary(&[0u8; 4]).err().expect("expected parse error").to_string();
        assert!(err.contains("too short"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_bad_magic() {
        let mut bin = build_binary(&[entry('a', 1, &[], &[], &[])]);
        bin[0] = b'X';
        let err = parse_binary(&bin).err().expect("expected parse error").to_string();
        assert!(err.contains("magic"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_bad_version() {
        let mut bin = build_binary(&[entry('a', 1, &[], &[], &[])]);
        bin[4] = 99;
        let err = parse_binary(&bin).err().expect("expected parse error").to_string();
        assert!(err.contains("unsupported version"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_truncated_data() {
        let bin = build_binary(&[entry('字', 6, &["ジ"], &[], &["character"])]);
        // Cut off the last few bytes of the data blob.
        let truncated = &bin[..bin.len() - 4];
        let err = parse_binary(truncated).err().expect("expected parse error").to_string();
        assert!(err.contains("truncated"), "got: {err}");
    }

    /// End-to-end: init once and exercise the public lookup fns against the
    /// process-global cell. Other tests in this file work on `parse_binary`
    /// directly so they don't fight over the OnceCell.
    #[test]
    fn init_and_lookup() {
        let entries = [
            entry('漢', 13, &["カン"], &[], &["Sino-"]),
            entry('字', 6, &["ジ"], &[], &["character"]),
        ];
        let bin = build_binary(&entries);
        init_from_bytes(&bin).expect("init");
        assert!(is_loaded());

        let kan = lookup_one('漢').expect("漢 present");
        assert_eq!(kan.literal, '漢');
        assert_eq!(kan.stroke_count, 13);
        assert_eq!(kan.on_readings, vec!["カン".to_string()]);

        // ASCII char is not kanji → not present, returns None.
        assert!(lookup_one('a').is_none());

        // Mix kanji + kana + ascii; only kanji entries that exist come back.
        let many = lookup_many("漢a字ぁ");
        let chars: Vec<char> = many.iter().map(|e| e.literal).collect();
        assert_eq!(chars, vec!['漢', '字']);
    }
}
