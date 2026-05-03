use anyhow::Result;
use kanjidic_types::KanjiEntry;
use std::io::Write;
use std::path::Path;

// Binary format: kanjidic.bin
//
//  [4]  magic       b"KDIC"
//  [1]  version     1u8
//  [4]  count       u32 LE — number of index entries
//  [N×8] index      sorted (codepoint: u32 LE, byte_offset: u32 LE) pairs
//  [4]  data_len    u32 LE
//  […]  data_blob   length-prefixed postcard entries (u32 LE len + bytes)

pub fn write_index(entries: &[KanjiEntry], output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Sort by codepoint for binary search.
    let mut sorted: Vec<&KanjiEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.literal as u32);

    // Serialize entries into the data blob, recording byte offsets.
    let mut data_blob: Vec<u8> = Vec::new();
    let mut index: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());

    for entry in &sorted {
        let offset = data_blob.len() as u32;
        index.push((entry.literal as u32, offset));

        let serialized = postcard::to_allocvec(entry)?;
        let len = serialized.len() as u32;
        data_blob.extend_from_slice(&len.to_le_bytes());
        data_blob.extend_from_slice(&serialized);
    }

    // Write the file.
    let file = std::fs::File::create(output)?;
    let mut w = std::io::BufWriter::new(file);

    // Header
    w.write_all(b"KDIC")?;
    w.write_all(&[1u8])?;

    // Index
    w.write_all(&(index.len() as u32).to_le_bytes())?;
    for (cp, off) in &index {
        w.write_all(&cp.to_le_bytes())?;
        w.write_all(&off.to_le_bytes())?;
    }

    // Data blob
    w.write_all(&(data_blob.len() as u32).to_le_bytes())?;
    w.write_all(&data_blob)?;

    Ok(())
}
