//! Pure-Rust examples (Tatoeba-style sentences) runtime. `examples-wasm` is
//! a thin `#[wasm_bindgen]` shim on top of this.

use anyhow::{anyhow, bail, Context};
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
        Ok(i) => match dict.index.get(i) {
            Some((_, offsets)) => offsets
                .iter()
                .take(max)
                .filter_map(|&off| get_sentence(&dict.sentences_bytes, off))
                .collect(),
            None => vec![],
        },
    }
}

fn get_sentence(bytes: &[u8], offset: u32) -> Option<ExampleEntry> {
    let pos = offset as usize;
    let len = u32::from_le_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?) as usize;
    let start = pos + 4;
    from_bytes(bytes.get(start..start + len)?).ok()
}

fn parse_binary<'a>(bytes: &'a [u8]) -> anyhow::Result<ExamplesDictInner> {
    if bytes.len() < 9 {
        bail!("examples binary too short");
    }
    if bytes.get(0..4) != Some(b"EXPL".as_slice()) {
        bail!("invalid magic bytes");
    }
    let version = bytes.get(4).copied().unwrap_or(0);
    if version != 1 {
        bail!("unsupported version {version}");
    }

    let mut pos = 5usize;

    let read_u32 = |bytes: &[u8], pos: usize| -> anyhow::Result<usize> {
        let raw = bytes
            .get(pos..pos + 4)
            .with_context(|| format!("examples binary truncated reading length at {pos}"))?;
        Ok(u32::from_le_bytes(raw.try_into()?) as usize)
    };
    let read_slice = |bytes: &'a [u8], pos: usize, len: usize, what: &str| -> anyhow::Result<&'a [u8]> {
        bytes.get(pos..pos + len).with_context(|| {
            format!("examples binary truncated reading {what} ({len} bytes at {pos})")
        })
    };

    let index_len = read_u32(bytes, pos)?;
    pos += 4;
    let index: Vec<(String, Vec<u32>)> = from_bytes(read_slice(bytes, pos, index_len, "index")?)?;
    pos += index_len;

    let sentences_len = read_u32(bytes, pos)?;
    pos += 4;
    let sentences_bytes = read_slice(bytes, pos, sentences_len, "sentences")?.to_vec();

    Ok(ExamplesDictInner {
        index,
        sentences_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use examples_types::ExampleEntry;
    use postcard::to_allocvec;

    /// Build a small EXPL binary in-memory. `index` is (headword → ordered list
    /// of sentence indices into `sentences`), so the test can control which
    /// example offsets each headword resolves to.
    fn build_binary(sentences: &[ExampleEntry], index: &[(&str, Vec<usize>)]) -> Vec<u8> {
        let mut sentences_bytes = Vec::new();
        let mut offsets = Vec::with_capacity(sentences.len());
        for s in sentences {
            offsets.push(sentences_bytes.len() as u32);
            let ser = to_allocvec(s).unwrap();
            sentences_bytes.extend_from_slice(&(ser.len() as u32).to_le_bytes());
            sentences_bytes.extend_from_slice(&ser);
        }

        // Lookup uses `binary_search_by`, so the index must be sorted by key.
        let mut idx: Vec<(String, Vec<u32>)> = index
            .iter()
            .map(|(k, ids)| (k.to_string(), ids.iter().map(|&i| offsets[i]).collect()))
            .collect();
        idx.sort_by(|a, b| a.0.cmp(&b.0));

        let index_bytes = to_allocvec(&idx).unwrap();

        let mut out = Vec::new();
        out.extend_from_slice(b"EXPL");
        out.push(1u8);
        out.extend_from_slice(&(index_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&index_bytes);
        out.extend_from_slice(&(sentences_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&sentences_bytes);
        out
    }

    fn ex(j: &str, e: &str) -> ExampleEntry {
        ExampleEntry {
            japanese: j.into(),
            english: e.into(),
        }
    }

    #[test]
    fn parse_binary_roundtrip() {
        let bin = build_binary(
            &[ex("a", "A"), ex("b", "B")],
            &[("alpha", vec![0]), ("beta", vec![1])],
        );
        let inner = parse_binary(&bin).expect("parse");
        assert_eq!(inner.index.len(), 2);
        assert_eq!(inner.index[0].0, "alpha");
        assert_eq!(inner.index[1].0, "beta");
    }

    #[test]
    fn parse_binary_rejects_short_input() {
        let err = parse_binary(&[0u8; 4])
            .err()
            .expect("expected error")
            .to_string();
        assert!(err.contains("too short"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_bad_magic() {
        let mut bin = build_binary(&[ex("x", "X")], &[("k", vec![0])]);
        bin[0] = b'Q';
        let err = parse_binary(&bin).err().expect("expected error").to_string();
        assert!(err.contains("magic"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_bad_version() {
        let mut bin = build_binary(&[ex("x", "X")], &[("k", vec![0])]);
        bin[4] = 7;
        let err = parse_binary(&bin).err().expect("expected error").to_string();
        assert!(err.contains("unsupported version"), "got: {err}");
    }

    #[test]
    fn parse_binary_rejects_truncated_sentences() {
        let bin = build_binary(&[ex("x", "X")], &[("k", vec![0])]);
        let cut = &bin[..bin.len() - 2];
        let err = parse_binary(cut).err().expect("expected error").to_string();
        assert!(err.contains("truncated"), "got: {err}");
    }

    /// End-to-end: init the OnceCell once and exercise `lookup`. The other
    /// tests in this file work on `parse_binary` directly so they don't fight
    /// over the process-global cell.
    #[test]
    fn init_and_lookup_respects_max() {
        let sentences = [
            ex("s0", "zero"),
            ex("s1", "one"),
            ex("s2", "two"),
            ex("s3", "three"),
        ];
        let bin = build_binary(
            &sentences,
            &[
                ("noun", vec![0, 1, 2, 3]),
                ("verb", vec![3]),
            ],
        );
        init_from_bytes(&bin).expect("init");
        assert!(is_loaded());

        // max caps the number of returned entries.
        let two = lookup("noun", 2);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].japanese, "s0");
        assert_eq!(two[1].japanese, "s1");

        // max greater than available returns all available.
        let all = lookup("noun", 100);
        assert_eq!(all.len(), 4);

        // Unknown headword returns empty (binary_search miss path).
        assert!(lookup("missing", 5).is_empty());

        // Single-entry headword works.
        let v = lookup("verb", 5);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].english, "three");
    }
}
