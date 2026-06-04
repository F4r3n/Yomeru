use crate::indexer::DictionaryIndex;
use anyhow::Result;
use std::io::Write;
use std::path::Path;

/// Binary format:
///
/// | Field                  | Size   | Notes                                |
/// |------------------------|--------|--------------------------------------|
/// | magic                  | 4      | b"JMDI"                              |
/// | version                | 1      | currently 3                          |
/// | fst_len                | 4 LE   | byte length of FST data              |
/// | fst_data               | fst_len|                                      |
/// | lookup_table_len       | 4 LE   | byte length of postcard lookup_table |
/// | lookup_table_data      | n      |                                      |
/// | entries_len            | 4 LE   | byte length of entries blob          |
/// | entries_data           | n      | length-prefixed postcard entries     |
/// | seq_index_len          | 4 LE   | byte length of seq index (v3+)       |
/// | seq_index_data         | n      | postcard Vec<(u32, u32)> sorted      |
pub fn write_index(index: &DictionaryIndex, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    f.write_all(b"JMDI")?;
    // v2: WordEntry now always carries ke_pri / re_pri priorities,
    //     ke_inf info, and sense misc tags (previously feature-gated).
    // v3: appended sequence-index section for ent_seq -> byte_offset lookup.
    f.write_all(&[3u8])?; // version

    let fst_len = index.fst_bytes.len() as u32;
    f.write_all(&fst_len.to_le_bytes())?;
    f.write_all(&index.fst_bytes)?;

    let lt_len = index.lookup_table_bytes.len() as u32;
    f.write_all(&lt_len.to_le_bytes())?;
    f.write_all(&index.lookup_table_bytes)?;

    let entries_len = index.entries_bytes.len() as u32;
    f.write_all(&entries_len.to_le_bytes())?;
    f.write_all(&index.entries_bytes)?;

    let seq_len = index.seq_index_bytes.len() as u32;
    f.write_all(&seq_len.to_le_bytes())?;
    f.write_all(&index.seq_index_bytes)?;

    Ok(())
}
