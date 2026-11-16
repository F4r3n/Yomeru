//! A loaded `grammar.bin` and its lookups.

use fst::{IntoStreamer, Streamer};

use crate::format;
use crate::types::GrammarEntry;

/// The longest pattern a lookup inside text tries, in chars.
const MAX_PATTERN_CHARS: usize = 24;

/// Shortest match and pattern the hover popup offers: one-char ones are
/// particles and endings (は, た) that would come up on almost every word.
pub const HOVER_MIN_CHARS: usize = 2;

pub struct GrammarDict {
    fst: fst::Map<Vec<u8>>,
    groups: Vec<Vec<u32>>,
    /// Sorted by id.
    ids: Vec<(String, u32)>,
    entries: Vec<u8>,
}

impl GrammarDict {
    /// Load `grammar.bin` bytes, as [`crate::compile`] wrote them.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        // Structure only: `compile` already decoded every entry of the file
        // it wrote, and the version byte refuses files of other builds.
        let parts = format::read(bytes)?;
        Ok(Self {
            fst: fst::Map::new(parts.fst.to_vec())?,
            groups: parts.groups,
            ids: parts.ids,
            entries: parts.entries.to_vec(),
        })
    }

    /// Number of grammar points.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Number of distinct patterns and forms.
    pub fn key_count(&self) -> usize {
        self.fst.len()
    }

    /// The entry with this id.
    pub fn by_id(&self, id: &str) -> Option<GrammarEntry> {
        let i = self
            .ids
            .binary_search_by(|(k, _)| k.as_str().cmp(id))
            .ok()?;
        let &(_, offset) = self.ids.get(i)?;
        format::entry_at(&self.entries, offset).ok()
    }

    /// Entries whose pattern or a form is exactly `text`, with their
    /// homographs.
    pub fn lookup(&self, text: &str) -> Vec<GrammarEntry> {
        self.fst
            .get(text)
            .map(|g| self.group(g))
            .unwrap_or_default()
    }

    /// The longest prefix of `text` (at most `max_chars` chars) that is a
    /// pattern or form, as written or once deinflected. Returns its entries
    /// and the prefix's length in chars.
    pub fn lookup_longest_match(
        &self,
        text: &str,
        max_chars: usize,
    ) -> Option<(Vec<GrammarEntry>, usize)> {
        self.longest_match_where(text, max_chars, |_, _| true)
    }

    /// The hover popup's lookup: [`Self::lookup_longest_match`] without the
    /// entries kept out of hover (`hover = false`) and without matches or
    /// patterns under [`HOVER_MIN_CHARS`]. When every entry of the longest
    /// match is left out, a shorter match can still show.
    pub fn hover_match(&self, text: &str, max_chars: usize) -> Option<(Vec<GrammarEntry>, usize)> {
        self.longest_match_where(text, max_chars, |e, chars| {
            e.hover && chars >= HOVER_MIN_CHARS && e.pattern.chars().count() >= HOVER_MIN_CHARS
        })
    }

    fn longest_match_where(
        &self,
        text: &str,
        max_chars: usize,
        keep: impl Fn(&GrammarEntry, usize) -> bool,
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
                    let entries: Vec<GrammarEntry> = self
                        .group(g)
                        .into_iter()
                        .filter(|e| keep(e, chars))
                        .collect();
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
    /// 1. patterns or forms that are exactly `text`;
    /// 2. the pattern `text` is a conjugation of (ようにしなかった → ようにする);
    /// 3. patterns inside `text`, longest first: 来てよかった → てよかった.
    ///    A match lying within a longer one is left out (よかった within
    ///    てよかった), and so are one-char patterns, or every particle and
    ///    deinflected ending (った → つ) would come up;
    /// 4. patterns and forms starting with `text`.
    pub fn search(&self, text: &str, max: usize) -> Vec<GrammarEntry> {
        let text = text.trim();
        let mut out: Vec<GrammarEntry> = Vec::new();
        if text.is_empty() || max == 0 {
            return out;
        }
        let push = |entries: Vec<GrammarEntry>, out: &mut Vec<GrammarEntry>| {
            for e in entries {
                let dup = out.iter().any(|o| o.id == e.id);
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
                    .filter(|e| e.pattern.chars().count() >= 2)
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

    /// Entries with a pattern or form starting with `prefix`, for a search box.
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

    /// The entries of group `g`, then their homographs not in it.
    fn group(&self, g: u64) -> Vec<GrammarEntry> {
        let mut out: Vec<GrammarEntry> = self
            .groups
            .get(g as usize)
            .into_iter()
            .flatten()
            .filter_map(|&offset| format::entry_at(&self.entries, offset).ok())
            .collect();
        let missing: Vec<String> = out
            .iter()
            .flat_map(|e| &e.homographs)
            .filter(|h| !out.iter().any(|e| &e.id == *h))
            .cloned()
            .collect();
        for id in missing {
            if !out.iter().any(|e| e.id == id)
                && let Some(e) = self.by_id(&id)
            {
                out.push(e);
            }
        }
        out
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
