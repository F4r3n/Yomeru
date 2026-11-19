//! `grammar.bin`: `YGRM` magic + version byte, then four length-prefixed
//! (u32 LE) blobs:
//!
//! 1. FST: pattern or form → group index.
//! 2. Groups (postcard `Vec<Vec<u32>>`): group index → byte offsets of its
//!    entries in the entries blob, in dictionary order.
//! 3. Ids (postcard `Vec<(String, u32, u32)>`, sorted by id): id, `seq`,
//!    byte offset.
//! 4. Entries: each a u32 LE length, then a raw-deflated postcard
//!    [`GrammarEntry`]. Only a handful are read per lookup, so each is
//!    compressed on its own.

use std::io::Read;
#[cfg(feature = "build")]
use std::io::Write;

use anyhow::{Context, bail};
use flate2::read::DeflateDecoder;

use crate::types::GrammarEntry;

pub(crate) const MAGIC: &[u8; 4] = b"YGRM";
/// Bump on any change to the layout or to the types. The file ships with
/// the code, so a mismatch is a build mistake.
pub const FORMAT_VERSION: u8 = 11;

#[cfg(feature = "build")]
pub(crate) fn write(entries: &[GrammarEntry]) -> anyhow::Result<Vec<u8>> {
    use std::collections::BTreeMap;

    let mut blob = Vec::new();
    let mut keys: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    let mut ids: Vec<(String, u32, u32)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let offset = u32::try_from(blob.len()).context("grammar dictionary over 4 GB")?;
        let bytes = compress(&postcard::to_allocvec(entry)?)?;
        let back: GrammarEntry = postcard::from_bytes(&decompress(&bytes)?)
            .with_context(|| format!("entry {} doesn't decode", entry.id))?;
        if &back != entry {
            bail!("entry {} doesn't survive encoding", entry.id);
        }
        blob.extend_from_slice(&u32::try_from(bytes.len())?.to_le_bytes());
        blob.extend_from_slice(&bytes);
        ids.push((entry.id.clone(), entry.seq, offset));
        for key in entry.keys() {
            let group = keys.entry(key).or_default();
            if !group.contains(&offset) {
                group.push(offset);
            }
        }
    }
    ids.sort();

    let mut fst = fst::MapBuilder::memory();
    let mut groups = Vec::with_capacity(keys.len());
    for (i, (key, offsets)) in keys.into_iter().enumerate() {
        fst.insert(key, i as u64)?;
        groups.push(offsets);
    }
    let fst = fst.into_inner()?;

    let mut out = Vec::with_capacity(blob.len() + fst.len() + 1024);
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    for part in [
        fst.as_slice(),
        &postcard::to_allocvec(&groups)?,
        &postcard::to_allocvec(&ids)?,
        &blob,
    ] {
        out.extend_from_slice(&u32::try_from(part.len())?.to_le_bytes());
        out.extend_from_slice(part);
    }
    Ok(out)
}

pub(crate) struct Parts<'a> {
    pub fst: &'a [u8],
    pub groups: Vec<Vec<u32>>,
    pub ids: Vec<(String, u32, u32)>,
    pub entries: &'a [u8],
}

pub(crate) fn read(bytes: &[u8]) -> anyhow::Result<Parts<'_>> {
    if bytes.get(..4) != Some(MAGIC.as_slice()) {
        bail!("not a grammar dictionary");
    }
    let version = bytes.get(4).copied().unwrap_or(0);
    if version != FORMAT_VERSION {
        bail!(
            "grammar dictionary format {version}, this version reads {FORMAT_VERSION}: rebuild it (cargo xtask build-grammar)"
        );
    }
    let mut rest = bytes.get(5..).unwrap_or_default();
    let mut next = |what: &str| -> anyhow::Result<&[u8]> {
        let (len, after) = split_len(rest).with_context(|| format!("truncated {what}"))?;
        let part = after
            .get(..len)
            .with_context(|| format!("truncated {what}"))?;
        rest = after.get(len..).unwrap_or_default();
        Ok(part)
    };
    let fst = next("fst")?;
    let groups: Vec<Vec<u32>> = postcard::from_bytes(next("groups")?).context("bad groups")?;
    let ids: Vec<(String, u32, u32)> = postcard::from_bytes(next("ids")?).context("bad ids")?;
    let entries = next("entries")?;
    check_offsets(
        groups.iter().flatten().chain(ids.iter().map(|(_, _, o)| o)),
        entries,
    )?;
    Ok(Parts {
        fst,
        groups,
        ids,
        entries,
    })
}

/// Every offset must start a whole entry, so lookups can't read past the
/// blob or from the middle of an entry.
fn check_offsets<'a>(offsets: impl Iterator<Item = &'a u32>, entries: &[u8]) -> anyhow::Result<()> {
    let mut starts = Vec::new();
    let mut pos = 0usize;
    while pos < entries.len() {
        starts.push(pos);
        let (len, _) = split_len(entries.get(pos..).unwrap_or_default())
            .with_context(|| format!("truncated entry at {pos}"))?;
        pos = pos
            .checked_add(4 + len)
            .filter(|&end| end <= entries.len())
            .with_context(|| format!("truncated entry at {pos}"))?;
    }
    for &offset in offsets {
        if starts.binary_search(&(offset as usize)).is_err() {
            bail!("offset into the middle of an entry ({offset})");
        }
    }
    Ok(())
}

/// The entry at byte `offset` of the entries blob.
pub(crate) fn entry_at(entries: &[u8], offset: u32) -> anyhow::Result<GrammarEntry> {
    let (len, after) = split_len(entries.get(offset as usize..).unwrap_or_default())
        .context("bad entry offset")?;
    let packed = after.get(..len).context("truncated entry")?;
    Ok(postcard::from_bytes(&decompress(packed)?)?)
}

fn split_len(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let (len, rest) = bytes.split_first_chunk::<4>()?;
    Some((u32::from_le_bytes(*len) as usize, rest))
}

#[cfg(feature = "build")]
fn compress(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(bytes)?;
    Ok(enc.finish()?)
}

fn decompress(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    DeflateDecoder::new(bytes).read_to_end(&mut out)?;
    Ok(out)
}
