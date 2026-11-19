//! A loaded `grammar.bin` and its lookups.

use std::collections::HashMap;

use fst::{IntoStreamer, Streamer};

use crate::format;
use crate::types::GrammarEntry;

/// The longest pattern a lookup inside text tries, in chars.
pub(crate) const MAX_PATTERN_CHARS: usize = 24;

/// Shortest match and pattern the hover popup offers: one-char ones are
/// particles and endings (は, た) that would come up on almost every word.
pub const HOVER_MIN_CHARS: usize = 2;

/// The longest word an お〜 / ご〜 pattern's 〜 stands for, in chars.
const MAX_STEM_CHARS: usize = 6;

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '々')
}

/// A ます-stem after お: kanji, then at most three kana, the last an i- or
/// e-row sound (待ち, 出かけ, 見え).
fn is_verb_stem(stem: &str) -> bool {
    let kana: String = stem.chars().skip_while(|&c| is_kanji(c)).collect();
    let head = stem.chars().count() - kana.chars().count();
    head > 0
        && (1..=3).contains(&kana.chars().count())
        && kana.chars().all(|c| matches!(c, 'ぁ'..='ゖ'))
        && kana
            .chars()
            .last()
            .is_some_and(|c| "いきぎしじちぢにひびぴみりえけげせぜてでねへべぺめれ".contains(c))
}

pub struct GrammarDict {
    fst: fst::Map<Vec<u8>>,
    groups: Vec<Vec<u32>>,
    /// `(id, seq, offset)`, sorted by id.
    ids: Vec<(String, u32, u32)>,
    /// Each entry's `seq` → its byte offset.
    seqs: HashMap<u32, u32>,
    entries: Vec<u8>,
}

impl GrammarDict {
    /// Load `grammar.bin` bytes, as [`crate::compile`] wrote them.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        // Structure only: `compile` already decoded every entry of the file
        // it wrote, and the version byte refuses files of other builds.
        let parts = format::read(bytes)?;
        let seqs = parts
            .ids
            .iter()
            .map(|&(_, seq, offset)| (seq, offset))
            .collect();
        Ok(Self {
            fst: fst::Map::new(parts.fst.to_vec())?,
            groups: parts.groups,
            ids: parts.ids,
            seqs,
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
            .binary_search_by(|(k, _, _)| k.as_str().cmp(id))
            .ok()?;
        let &(_, _, offset) = self.ids.get(i)?;
        format::entry_at(&self.entries, offset).ok()
    }

    /// Every entry, in id order. Decodes each: for building an index once,
    /// not for a lookup.
    pub fn entries(&self) -> impl Iterator<Item = GrammarEntry> + '_ {
        self.ids
            .iter()
            .filter_map(|&(_, _, offset)| format::entry_at(&self.entries, offset).ok())
    }

    /// The entry numbered `seq`, which a grammar card reviews. `None` once
    /// the point left the dictionary.
    pub fn by_seq(&self, seq: u32) -> Option<GrammarEntry> {
        let &offset = self.seqs.get(&seq)?;
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

    /// Every match at the start of `text`, longest first: each prefix (at
    /// most `max_chars` chars) that is a pattern or form, as written or once
    /// deinflected, with its entries and length in chars; and the
    /// honorific match ([`Self::honorific_match`]) if there is one. For a
    /// caller that knows where words end and wants the longest match ending
    /// on one.
    pub fn matches_at(&self, text: &str, max_chars: usize) -> Vec<(Vec<GrammarEntry>, usize)> {
        self.matches_at_where(text, max_chars, |_, _| true)
    }

    /// [`Self::matches_at`] keeping only the entries `keep(entry, chars)`
    /// accepts: one it rejects leaves the lookup to the text's other
    /// readings (にしろ as にする's command) and shorter matches.
    pub fn matches_at_where(
        &self,
        text: &str,
        max_chars: usize,
        keep: impl Fn(&GrammarEntry, usize) -> bool,
    ) -> Vec<(Vec<GrammarEntry>, usize)> {
        let mut out = self.plain_matches(text, max_chars, &keep, false);
        if let Some(h) = self.honorific_match(text, max_chars, &keep)
            && !out.iter().any(|(_, n)| *n == h.1)
        {
            out.push(h);
            out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        }
        out
    }

    /// The longer of the match as written and the honorific one
    /// ([`Self::honorific_match`]).
    pub(crate) fn longest_match_where(
        &self,
        text: &str,
        max_chars: usize,
        keep: impl Fn(&GrammarEntry, usize) -> bool,
    ) -> Option<(Vec<GrammarEntry>, usize)> {
        let plain = self.plain_match(text, max_chars, &keep);
        match self.honorific_match(text, max_chars, &keep) {
            Some(h) if plain.as_ref().is_none_or(|p| h.1 > p.1) => Some(h),
            _ => plain,
        }
    }

    /// お〜 / ご〜 patterns (お〜です, お〜になる) hold a 〜 where text has a
    /// word: お待ちです tries お〜です. The word after お is a verb stem,
    /// kanji then kana ending in an i- or e-row sound (待ち, 出かけ), so
    /// お茶です or おいしいです don't match; after ご it is a kanji compound
    /// (利用). ご + noun + です is left out: ご家族です is just a noun.
    fn honorific_match(
        &self,
        text: &str,
        max_chars: usize,
        keep: &impl Fn(&GrammarEntry, usize) -> bool,
    ) -> Option<(Vec<GrammarEntry>, usize)> {
        let mut chars = text.char_indices();
        let (_, prefix) = chars.next()?;
        if prefix != 'お' && prefix != 'ご' {
            return None;
        }
        let mut best: Option<(Vec<GrammarEntry>, usize)> = None;
        // `stem_len`: chars between お and `i`.
        for (stem_len, (i, _)) in chars.enumerate().take(MAX_STEM_CHARS + 1) {
            let (Some(stem), Some(rest)) = (text.get(prefix.len_utf8()..i), text.get(i..)) else {
                continue;
            };
            let ok = match prefix {
                'お' => is_verb_stem(stem),
                _ => {
                    stem.chars().count() >= 2
                        && stem.chars().all(is_kanji)
                        && !rest.starts_with(['で', 'だ'])
                }
            };
            if !ok {
                continue;
            }
            // お〜 + rest: a match of n chars covers n - 1 + stem_len of text.
            let keyed = format!("{prefix}〜{rest}");
            let Some((entries, n)) =
                self.plain_match(&keyed, (max_chars + 1).saturating_sub(stem_len), &|e, n| {
                    keep(e, n + stem_len - 1)
                })
            else {
                continue;
            };
            // It must reach past the 〜 into `rest`.
            if n <= 2 {
                continue;
            }
            let len = n + stem_len - 1;
            if best.as_ref().is_none_or(|b| len > b.1) {
                best = Some((entries, len));
            }
        }
        best
    }

    fn plain_match(
        &self,
        text: &str,
        max_chars: usize,
        keep: &impl Fn(&GrammarEntry, usize) -> bool,
    ) -> Option<(Vec<GrammarEntry>, usize)> {
        self.plain_matches(text, max_chars, keep, true)
            .into_iter()
            .next()
    }

    /// The prefixes of `text` that are keys, longest first; only the
    /// longest with `first_only`.
    fn plain_matches(
        &self,
        text: &str,
        max_chars: usize,
        keep: &impl Fn(&GrammarEntry, usize) -> bool,
        first_only: bool,
    ) -> Vec<(Vec<GrammarEntry>, usize)> {
        let boundaries: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .take(max_chars + 1)
            .collect();
        let reach = self.reach(text);
        let mut deinflector = deinflect::Deinflector::default();
        let mut out = Vec::new();

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
            // No pattern is a する noun: 以上にしろ (以上にする) isn't 以上に.
            for form in deinflector.run_without(surface, deinflect::Reason::SuruStem) {
                if let Some(g) = self.fst.get(form) {
                    let entries: Vec<GrammarEntry> = self
                        .group(g)
                        .into_iter()
                        .filter(|e| keep(e, chars))
                        .collect();
                    if !entries.is_empty() {
                        out.push((entries, chars));
                        if first_only {
                            return out;
                        }
                        break;
                    }
                }
            }
        }
        out
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
