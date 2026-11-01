//! Pure-Rust KANJIDIC runtime: binary loader + lookup. `kanjidic-wasm` is a
//! thin `#[wasm_bindgen]` shim on top of this.

use anyhow::{anyhow, bail, Context};
use japanese_utils::is_kanji;
use kanjidic_types::KanjiEntry;
use once_cell::sync::OnceCell;
use postcard::from_bytes;
use serde::Deserialize;

static KDICT: OnceCell<KanjiDictInner> = OnceCell::new();

struct KanjiDictInner {
    version: u8,
    index: Vec<(u32, u32)>,
    data: Vec<u8>,
}

/// Entry layout of version-1 binaries, from before KanjiVG parts. Still read
/// so a server whose dictionary volume predates parts keeps working.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct KanjiEntryV1 {
    literal: char,
    stroke_count: u8,
    grade: Option<u8>,
    freq: Option<u16>,
    jlpt: Option<u8>,
    on_readings: Vec<String>,
    kun_readings: Vec<String>,
    meanings: Vec<String>,
}

impl From<KanjiEntryV1> for KanjiEntry {
    fn from(e: KanjiEntryV1) -> Self {
        KanjiEntry {
            literal: e.literal,
            stroke_count: e.stroke_count,
            grade: e.grade,
            freq: e.freq,
            jlpt: e.jlpt,
            on_readings: e.on_readings,
            kun_readings: e.kun_readings,
            meanings: e.meanings,
            parts: vec![],
        }
    }
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
    KDICT.get()?.entry(codepoint)
}

impl KanjiDictInner {
    fn entry(&self, codepoint: u32) -> Option<KanjiEntry> {
        let pos = self
            .index
            .binary_search_by_key(&codepoint, |&(cp, _)| cp)
            .ok()?;
        let byte_offset = self.index.get(pos)?.1 as usize;
        let bytes = &self.data;
        let len =
            u32::from_le_bytes(bytes.get(byte_offset..byte_offset + 4)?.try_into().ok()?) as usize;
        let start = byte_offset + 4;
        let raw = bytes.get(start..start + len)?;
        if self.version == 1 {
            from_bytes::<KanjiEntryV1>(raw).ok().map(KanjiEntry::from)
        } else {
            from_bytes(raw).ok()
        }
    }
}

fn parse_binary(bytes: &[u8]) -> anyhow::Result<KanjiDictInner> {
    if bytes.len() < 9 {
        bail!("kanjidic binary too short");
    }
    if bytes.get(0..4) != Some(b"KDIC".as_slice()) {
        bail!("invalid magic bytes");
    }
    let version = bytes.get(4).copied().unwrap_or(0);
    if !matches!(version, 1 | 2) {
        bail!("unsupported version {version}");
    }

    let read_u32 = |bytes: &[u8], pos: usize| -> anyhow::Result<u32> {
        let raw = bytes
            .get(pos..pos + 4)
            .with_context(|| format!("kanjidic binary truncated at {pos}"))?;
        Ok(u32::from_le_bytes(raw.try_into()?))
    };

    let mut pos = 5usize;
    let count = read_u32(bytes, pos)? as usize;
    pos += 4;

    let mut index = Vec::with_capacity(count);
    for _ in 0..count {
        let cp = read_u32(bytes, pos)?;
        let off = read_u32(bytes, pos + 4)?;
        index.push((cp, off));
        pos += 8;
    }

    let data_len = read_u32(bytes, pos)? as usize;
    pos += 4;

    let data = bytes
        .get(pos..pos + data_len)
        .context("data blob truncated")?
        .to_vec();

    Ok(KanjiDictInner {
        version,
        index,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanjidic_types::KanjiEntry;
    use postcard::to_allocvec;

    fn entry(
        literal: char,
        strokes: u8,
        on: &[&str],
        kun: &[&str],
        meanings: &[&str],
    ) -> KanjiEntry {
        KanjiEntry {
            literal,
            stroke_count: strokes,
            grade: None,
            freq: None,
            jlpt: None,
            on_readings: on.iter().map(|s| s.to_string()).collect(),
            kun_readings: kun.iter().map(|s| s.to_string()).collect(),
            meanings: meanings.iter().map(|s| s.to_string()).collect(),
            parts: vec![],
        }
    }

    /// Serialize a small in-memory KDIC binary matching the format documented
    /// in `parse_binary`. The index is sorted by codepoint to match the real
    /// builder's invariant (`get_entry` does a binary search).
    fn build_binary(entries: &[KanjiEntry]) -> Vec<u8> {
        let encoded = entries
            .iter()
            .map(|e| (e.literal, to_allocvec(e).unwrap()))
            .collect();
        build_raw(2, encoded)
    }

    /// The binary for already-encoded `(literal, entry bytes)` pairs, so a
    /// test can lay out entries the way an older version did.
    fn build_raw(version: u8, mut entries: Vec<(char, Vec<u8>)>) -> Vec<u8> {
        entries.sort_by_key(|(c, _)| *c as u32);

        let mut out = Vec::new();
        out.extend_from_slice(b"KDIC");
        out.push(version);
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());

        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(entries.len());
        for (_, ser) in &entries {
            offsets.push(data.len() as u32);
            data.extend_from_slice(&(ser.len() as u32).to_le_bytes());
            data.extend_from_slice(ser);
        }
        for ((c, _), off) in entries.iter().zip(offsets.iter()) {
            out.extend_from_slice(&(*c as u32).to_le_bytes());
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
    fn version_1_entries_load_without_parts() {
        let v1 = KanjiEntryV1 {
            literal: '字',
            stroke_count: 6,
            grade: Some(1),
            freq: None,
            jlpt: None,
            on_readings: vec!["ジ".into()],
            kun_readings: vec![],
            meanings: vec!["character".into()],
        };
        let bin = build_raw(1, vec![('字', to_allocvec(&v1).unwrap())]);
        let dict = parse_binary(&bin).expect("parse");
        let e = dict.entry('字' as u32).expect("v1 entry decodes");
        assert_eq!(e.meanings, vec!["character".to_string()]);
        assert!(e.parts.is_empty());
    }

    #[test]
    fn version_2_entries_keep_their_parts() {
        let mut e = entry('語', 14, &["ゴ"], &[], &["word"]);
        e.parts = vec![kanjidic_types::KanjiPart {
            element: '言',
            original: None,
            radical: true,
            phonetic: false,
        }];
        let dict = parse_binary(&build_binary(std::slice::from_ref(&e))).expect("parse");
        assert_eq!(dict.entry('語' as u32), Some(e));
    }

    #[test]
    fn parse_binary_rejects_short_input() {
        let err = parse_binary(&[0u8; 4])
            .err()
            .expect("expected parse error")
            .to_string();
        assert!(err.contains("too short"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_bad_magic() {
        let mut bin = build_binary(&[entry('a', 1, &[], &[], &[])]);
        bin[0] = b'X';
        let err = parse_binary(&bin)
            .err()
            .expect("expected parse error")
            .to_string();
        assert!(err.contains("magic"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_bad_version() {
        let mut bin = build_binary(&[entry('a', 1, &[], &[], &[])]);
        bin[4] = 99;
        let err = parse_binary(&bin)
            .err()
            .expect("expected parse error")
            .to_string();
        assert!(err.contains("unsupported version"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_truncated_data() {
        let bin = build_binary(&[entry('字', 6, &["ジ"], &[], &["character"])]);
        // Cut off the last few bytes of the data blob.
        let truncated = &bin[..bin.len() - 4];
        let err = parse_binary(truncated)
            .err()
            .expect("expected parse error")
            .to_string();
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
