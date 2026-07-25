//! Build-time `disambig.bin` emitter.
//!
//! `examples-generator` needs to map a tokenized word to a JMdict ent_seq, but
//! it cannot link `jmdict-core` (that pulls in rkyv's `unaligned` feature, which
//! is incompatible with lindera's aligned rkyv in the same binary). So instead
//! of reading the rkyv archive at generation time, we precompute a plain,
//! rkyv-free side table here and let the generator `binary_search` it.
//!
//! Key is `(surface, reading)` so homographs disambiguate by their contextual
//! reading: lindera resolves e.g. 辛い → からい vs つらい, and that reading
//! selects the right sequence. The surface is the kanji form (or, for kana-only
//! words, the reading itself, since the base form lindera returns is kana).

use anyhow::Result;
use jmdict_types::{ReadingElement, WordEntry};
use postcard::to_allocvec;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// `(surface, reading) -> sorted unique ent_seq list`, sorted by key for
/// `binary_search`. A key maps to more than one sequence only for true
/// same-spelling, same-reading collisions; callers tiebreak by POS/frequency.
pub type DisambigTable = Vec<((String, String), Vec<u32>)>;

pub fn build_disambig(entries: &[WordEntry]) -> DisambigTable {
    let mut map: BTreeMap<(String, String), Vec<u32>> = BTreeMap::new();

    for entry in entries {
        let seq = entry.sequence;

        if entry.kanji_forms.is_empty() {
            // Kana-only word: lindera's base form is kana, so surface == reading.
            for r in &entry.reading_forms {
                push_unique(&mut map, (r.text.clone(), r.text.clone()), seq);
            }
            continue;
        }

        for k in &entry.kanji_forms {
            for r in &entry.reading_forms {
                if reading_applies_to(r, &k.text) {
                    push_unique(&mut map, (k.text.clone(), r.text.clone()), seq);
                }
            }
        }
    }

    map.into_iter().collect()
}

fn push_unique(map: &mut BTreeMap<(String, String), Vec<u32>>, key: (String, String), seq: u32) {
    let seqs = map.entry(key).or_default();
    // Sequences are produced in entry order; keep them sorted + unique so the
    // table is deterministic and the same word never lists a sequence twice.
    if let Err(pos) = seqs.binary_search(&seq) {
        seqs.insert(pos, seq);
    }
}

/// Whether a reading applies to a given kanji form. With JMdict's `re_restr`
/// data (the `full` feature) a reading may be restricted to specific kanji;
/// without it we conservatively pair every reading with every kanji form.
fn reading_applies_to(reading: &ReadingElement, kanji: &str) -> bool {
    #[cfg(feature = "full")]
    {
        reading.restricted_to.is_empty() || reading.restricted_to.iter().any(|k| k == kanji)
    }
    #[cfg(not(feature = "full"))]
    {
        let _ = (reading, kanji);
        true
    }
}

/// Binary format:
///
/// | Field      | Size  | Notes                                          |
/// |------------|-------|------------------------------------------------|
/// | magic      | 4     | b"DSMB"                                        |
/// | version    | 1     | currently 1                                    |
/// | table_len  | 4 LE  | byte length of the postcard table              |
/// | table_data | n     | postcard `Vec<((String, String), Vec<u32>)>`   |
pub fn write_disambig(table: &DisambigTable, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    f.write_all(b"DSMB")?;
    f.write_all(&[1u8])?; // version

    let table_bytes = to_allocvec(table)?;
    f.write_all(&(table_bytes.len() as u32).to_le_bytes())?;
    f.write_all(&table_bytes)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmdict_types::{KanjiElement, ReadingElement, WordEntry};

    fn entry(seq: u32, kanji: &[&str], readings: &[&str]) -> WordEntry {
        WordEntry {
            sequence: seq,
            kanji_forms: kanji.iter().map(|k| KanjiElement::from_text(*k)).collect(),
            reading_forms: readings
                .iter()
                .map(|r| ReadingElement::from_reading(*r))
                .collect(),
            senses: vec![],
        }
    }

    fn lookup<'a>(table: &'a DisambigTable, surface: &str, reading: &str) -> Option<&'a [u32]> {
        let key = (surface.to_string(), reading.to_string());
        table
            .binary_search_by(|(k, _)| k.cmp(&key))
            .ok()
            .map(|i| table[i].1.as_slice())
    }

    #[test]
    fn homographs_disambiguate_by_reading() {
        // 辛い: からい (spicy) vs つらい (painful) — two sequences, one spelling.
        let entries = vec![
            entry(1000, &["辛い"], &["からい"]),
            entry(2000, &["辛い"], &["つらい"]),
        ];
        let table = build_disambig(&entries);

        assert_eq!(lookup(&table, "辛い", "からい"), Some([1000u32].as_slice()));
        assert_eq!(lookup(&table, "辛い", "つらい"), Some([2000u32].as_slice()));
    }

    #[test]
    fn kana_only_word_keys_on_reading() {
        let entries = vec![entry(3000, &[], &["きれい"])];
        let table = build_disambig(&entries);
        assert_eq!(lookup(&table, "きれい", "きれい"), Some([3000u32].as_slice()));
    }

    #[test]
    fn table_is_sorted_for_binary_search() {
        let entries = vec![
            entry(1, &["猫"], &["ねこ"]),
            entry(2, &["犬"], &["いぬ"]),
            entry(3, &["鳥"], &["とり"]),
        ];
        let table = build_disambig(&entries);
        let keys: Vec<&(String, String)> = table.iter().map(|(k, _)| k).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "table must be sorted");
    }

    #[test]
    fn same_key_collects_multiple_sequences_sorted_unique() {
        // Pathological: same spelling AND reading across two sequences.
        let entries = vec![
            entry(50, &["生"], &["せい"]),
            entry(40, &["生"], &["せい"]),
            entry(50, &["生"], &["せい"]), // duplicate sequence must fold
        ];
        let table = build_disambig(&entries);
        assert_eq!(lookup(&table, "生", "せい"), Some([40u32, 50u32].as_slice()));
    }
}
