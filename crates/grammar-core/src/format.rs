//! `grammar.bin`: `YGRM` magic + version byte, then four length-prefixed
//! (u32 LE) blobs:
//!
//! 1. Meta: postcard [`GrammarMeta`].
//! 2. FST: term or reading → group index.
//! 3. Groups (postcard `Vec<Vec<u32>>`): group index → byte offsets of its
//!    entries in the entries blob, in dictionary order.
//! 4. Entries: each a u32 LE length, then a raw-deflated postcard
//!    [`GrammarEntry`]. Entries are big (a few KB of prose each) and only a
//!    handful are read per lookup, so each is compressed on its own.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use anyhow::{Context, bail};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use crate::types::{GrammarEntry, GrammarMeta};

pub(crate) const MAGIC: &[u8; 4] = b"YGRM";
/// Bump on any change to the layout or to the types: a stored file of
/// another version is refused, and the app asks for a new import.
pub const FORMAT_VERSION: u8 = 2;

/// An entry compressed for the entries blob, with the keys it's found by.
pub(crate) struct Packed {
    pub term: String,
    pub reading: String,
    pub bytes: Vec<u8>,
}

/// Compresses `entry`, and decodes the result once: an entry that doesn't
/// round-trip would otherwise only go missing at lookup.
pub(crate) fn pack(entry: &GrammarEntry) -> anyhow::Result<Packed> {
    let bytes = compress(&postcard::to_allocvec(entry)?)?;
    let back: GrammarEntry = postcard::from_bytes(&decompress(&bytes)?)
        .with_context(|| format!("entry {} doesn't decode", entry.term))?;
    if &back != entry {
        bail!("entry {} doesn't survive encoding", entry.term);
    }
    Ok(Packed {
        term: entry.term.clone(),
        reading: entry.reading.clone(),
        bytes,
    })
}

pub(crate) fn write(meta: &GrammarMeta, entries: &[Packed]) -> anyhow::Result<Vec<u8>> {
    let mut blob = Vec::new();
    let mut keys: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for entry in entries {
        let offset = u32::try_from(blob.len()).context("grammar dictionary over 4 GB")?;
        blob.extend_from_slice(&u32::try_from(entry.bytes.len())?.to_le_bytes());
        blob.extend_from_slice(&entry.bytes);

        for key in [entry.term.as_str(), entry.reading.as_str()] {
            if key.is_empty() {
                continue;
            }
            let group = keys.entry(key).or_default();
            if !group.contains(&offset) {
                group.push(offset);
            }
        }
    }

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
        postcard::to_allocvec(meta)?.as_slice(),
        &fst,
        &postcard::to_allocvec(&groups)?,
        &blob,
    ] {
        out.extend_from_slice(&u32::try_from(part.len())?.to_le_bytes());
        out.extend_from_slice(part);
    }
    Ok(out)
}

pub(crate) struct Parts<'a> {
    pub meta: GrammarMeta,
    pub fst: &'a [u8],
    pub groups: Vec<Vec<u32>>,
    pub entries: &'a [u8],
}

pub(crate) fn read(bytes: &[u8]) -> anyhow::Result<Parts<'_>> {
    if bytes.get(..4) != Some(MAGIC.as_slice()) {
        bail!("not a grammar dictionary");
    }
    let version = bytes.get(4).copied().unwrap_or(0);
    if version != FORMAT_VERSION {
        bail!(
            "grammar dictionary format {version}, this version reads {FORMAT_VERSION}: import it again"
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
    let meta = postcard::from_bytes(next("meta")?).context("bad meta")?;
    let fst = next("fst")?;
    let groups: Vec<Vec<u32>> = postcard::from_bytes(next("groups")?).context("bad groups")?;
    let entries = next("entries")?;
    check_offsets(&groups, entries)?;
    Ok(Parts {
        meta,
        fst,
        groups,
        entries,
    })
}

/// Every group offset must start a whole entry, so lookups can't read past
/// the blob or from the middle of an entry.
fn check_offsets(groups: &[Vec<u32>], entries: &[u8]) -> anyhow::Result<()> {
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
    for &offset in groups.iter().flatten() {
        if starts.binary_search(&(offset as usize)).is_err() {
            bail!("group points into the middle of an entry ({offset})");
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

fn compress(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(bytes)?;
    Ok(enc.finish()?)
}

fn decompress(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    DeflateDecoder::new(bytes).read_to_end(&mut out)?;
    Ok(out)
}
