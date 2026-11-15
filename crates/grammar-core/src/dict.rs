//! A loaded `grammar.bin` and its lookups.
//!
//! An owned value rather than a process global like the JMdict one: the
//! user imports, updates and removes this dictionary while the app runs.

use fst::{IntoStreamer, Streamer};

use crate::format;
use crate::types::{GrammarEntry, GrammarMeta};

/// The longest pattern a lookup inside text tries, in chars.
const MAX_PATTERN_CHARS: usize = 24;

pub struct GrammarDict {
    meta: GrammarMeta,
    fst: fst::Map<Vec<u8>>,
    groups: Vec<Vec<u32>>,
    entries: Vec<u8>,
}

impl GrammarDict {
    /// Load `grammar.bin` bytes, as [`crate::convert`] wrote them.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        // Structure only: `convert` already decoded every entry of the file
        // it wrote, and the version byte refuses files of other builds.
        let parts = format::read(bytes)?;
        Ok(Self {
            meta: parts.meta,
            fst: fst::Map::new(parts.fst.to_vec())?,
            groups: parts.groups,
            entries: parts.entries.to_vec(),
        })
    }

    pub fn meta(&self) -> &GrammarMeta {
        &self.meta
    }

    /// Number of distinct headwords and readings.
    pub fn key_count(&self) -> usize {
        self.fst.len()
    }

    /// Entries whose term or reading is exactly `text`.
    pub fn lookup(&self, text: &str) -> Vec<GrammarEntry> {
        self.fst
            .get(text)
            .map(|g| self.group(g))
            .unwrap_or_default()
    }

    /// Hover lookup: the longest prefix of `text` (at most `max_chars`
    /// chars) that is a headword, as written or once deinflected. Returns
    /// its entries and the prefix's length in chars.
    pub fn lookup_longest_match(
        &self,
        text: &str,
        max_chars: usize,
    ) -> Option<(Vec<GrammarEntry>, usize)> {
        let boundaries: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .take(max_chars + 1)
            .collect();
        let reach = self.reach(text);
        let mut deinflector = deinflect::Deinflector::default();

        for (chars, &end) in boundaries.iter().enumerate().rev() {
            if end == 0 {
                continue;
            }
            let Some(surface) = text.get(..end) else {
                continue;
            };
            // No key starts like this prefix, and no rule rewrites its
            // ending: none of its forms can be a key.
            if end > reach && !deinflect::can_deinflect(surface) {
                continue;
            }
            for form in deinflector.run(surface) {
                if let Some(g) = self.fst.get(form) {
                    let entries = self.group(g);
                    if !entries.is_empty() {
                        return Some((entries, chars));
                    }
                }
            }
        }
        None
    }

    /// What a search box shows for `text`, at most `max` entries, each once:
    ///
    /// 1. headwords or readings that are exactly `text`;
    /// 2. the headword `text` is a conjugation of (ようにしなかった → ようにする);
    /// 3. patterns inside `text`, longest first: 来てよかった → てよかった.
    ///    A match lying within a longer one is left out (よかった within
    ///    てよかった), and so are one-char headwords, or every particle and
    ///    deinflected ending (った → つ) would come up;
    /// 4. headwords starting with `text`.
    pub fn search(&self, text: &str, max: usize) -> Vec<GrammarEntry> {
        let text = text.trim();
        let mut out: Vec<GrammarEntry> = Vec::new();
        if text.is_empty() || max == 0 {
            return out;
        }
        let push = |entries: Vec<GrammarEntry>, out: &mut Vec<GrammarEntry>| {
            for e in entries {
                let dup = out
                    .iter()
                    .any(|o| o.term == e.term && o.website == e.website && o.meaning == e.meaning);
                if !dup && out.len() < max {
                    out.push(e);
                }
            }
        };

        let chars = text.chars().count();
        // Char spans of `text` already shown: (start, len).
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let exact = self.lookup(text);
        if !exact.is_empty() {
            spans.push((0, chars));
        }
        push(exact, &mut out);
        if let Some((entries, len)) = self.lookup_longest_match(text, chars)
            && len == chars
        {
            spans.push((0, chars));
            push(entries, &mut out);
        }

        let mut inner: Vec<(usize, usize, Vec<GrammarEntry>)> = text
            .char_indices()
            .enumerate()
            .filter_map(|(start, (i, _))| {
                let (entries, len) =
                    self.lookup_longest_match(text.get(i..)?, MAX_PATTERN_CHARS)?;
                let entries: Vec<GrammarEntry> = entries
                    .into_iter()
                    .filter(|e| e.term.chars().count() >= 2)
                    .collect();
                (len >= 2 && !entries.is_empty()).then_some((start, len, entries))
            })
            .collect();
        inner.sort_by_key(|&(start, len, _)| (std::cmp::Reverse(len), start));
        for (start, len, entries) in inner {
            let within = spans
                .iter()
                .any(|&(s, l)| s <= start && start + len <= s + l);
            if within {
                continue;
            }
            spans.push((start, len));
            push(entries, &mut out);
        }

        if out.len() < max {
            push(self.lookup_prefix(text, max), &mut out);
        }
        out
    }

    /// Entries whose term or reading starts with `prefix`, for a search box.
    /// At most `max` entries, in key order.
    pub fn lookup_prefix(&self, prefix: &str, max: usize) -> Vec<GrammarEntry> {
        let matcher = fst::automaton::Str::new(prefix);
        let mut stream = self
            .fst
            .search(fst::Automaton::starts_with(matcher))
            .into_stream();
        let mut seen = Vec::new();
        let mut out = Vec::new();
        while let Some((_, g)) = stream.next() {
            for &offset in self.groups.get(g as usize).into_iter().flatten() {
                if out.len() >= max {
                    return out;
                }
                if seen.contains(&offset) {
                    continue;
                }
                seen.push(offset);
                if let Ok(e) = format::entry_at(&self.entries, offset) {
                    out.push(e);
                }
            }
        }
        out
    }

    fn group(&self, g: u64) -> Vec<GrammarEntry> {
        self.groups
            .get(g as usize)
            .into_iter()
            .flatten()
            .filter_map(|&offset| format::entry_at(&self.entries, offset).ok())
            .collect()
    }

    /// How many bytes of `text` some key starts with.
    fn reach(&self, text: &str) -> usize {
        let fst = self.fst.as_fst();
        let mut node = fst.root();
        for (i, &b) in text.as_bytes().iter().enumerate() {
            let Some(t) = node.find_input(b) else {
                return i;
            };
            node = fst.node(node.transition(t).addr);
        }
        text.len()
    }
}
