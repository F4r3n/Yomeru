//! The kanji finder's index: which parts each kanji holds, at any depth, and
//! which kanji hold each part.
//!
//! [`build_index`] runs in `kanjidic-build`, which stores the result in
//! `kanjidic.bin`. It also runs at load time for binaries older than version
//! 3, so both paths give the same index.

use std::collections::{HashMap, HashSet};

use kanjidic_types::{part_meanings, KanjiEntry, PartIndexRecord, PartInfo};

/// Stroke counts for parts KANJIDIC has no entry for, mostly radical
/// variants (⻌, ⻖, ⺌). Taken from KanjiVG: the count each part has most
/// often across the kanji that contain it. Without it these common parts
/// would fall out of the finder's stroke-count rows.
const VARIANT_STROKES: &[(char, u8)] = &[
    ('𠂊', 2),
    ('⺕', 3),
    ('龰', 4),
    ('⻌', 3),
    ('⺤', 4),
    ('⻖', 3),
    ('マ', 2),
    ('⺌', 3),
    ('⺨', 3),
    ('⻏', 3),
    ('龶', 4),
    ('习', 3),
    ('⻞', 9),
    ('⺍', 3),
    ('𡰪', 6),
    ('业', 5),
    ('㐭', 8),
    ('巜', 2),
    ('电', 5),
    ('夗', 5),
    ('𢆉', 5),
    ('㐫', 6),
    ('𠂇', 2),
    ('⺦', 3),
    ('㐄', 4),
    ('丆', 2),
    ('丷', 2),
    ('乡', 3),
    ('㑒', 8),
    ('倠', 10),
    ('⺷', 6),
    ('䏍', 6),
    ('𠫯', 8),
    ('𬀷', 11),
    ('堇', 11),
    ('戋', 6),
];

fn variant_strokes(p: char) -> Option<u8> {
    VARIANT_STROKES
        .iter()
        .find(|&&(c, _)| c == p)
        .map(|&(_, n)| n)
}

/// Compute the index from every entry, parts included.
pub fn build_index(entries: &[KanjiEntry]) -> PartIndexRecord {
    let by_literal: HashMap<char, &KanjiEntry> = entries.iter().map(|e| (e.literal, e)).collect();

    let mut kanji: Vec<char> = by_literal.keys().copied().collect();
    kanji.sort_unstable();
    let parts_of: Vec<Vec<char>> = kanji
        .iter()
        .map(|&k| {
            let mut all = vec![k];
            collect_parts(k, &by_literal, &mut all, 0);
            all.sort_unstable();
            all.dedup();
            all
        })
        .collect();

    // Best known first: KANJIDIC's frequency rank, then the kanji with no
    // rank by stroke count, so simple shapes come before obscure ones.
    let mut ranked: Vec<usize> = (0..kanji.len()).collect();
    ranked.sort_unstable_by_key(|&i| {
        let k = kanji.get(i).copied().unwrap_or_default();
        by_literal.get(&k).map_or((u16::MAX, u8::MAX, k), |e| {
            (e.freq.unwrap_or(u16::MAX), e.stroke_count, k)
        })
    });

    let mut by_part: HashMap<char, Vec<char>> = HashMap::new();
    let mut listed: HashSet<char> = HashSet::new();
    for i in ranked {
        let (Some(&k), Some(own)) = (kanji.get(i), parts_of.get(i)) else {
            continue;
        };
        for &p in own {
            by_part.entry(p).or_default().push(k);
            if p != k {
                listed.insert(p);
            }
        }
    }

    // The full form some kanji records for a variant (水 for 氵).
    let originals: HashMap<char, char> = entries
        .iter()
        .flat_map(|e| e.parts.iter())
        .filter_map(|p| Some((p.element, p.original?)))
        .collect();
    let meanings = |c: &char| {
        by_literal
            .get(c)
            .map(|e| part_meanings(&e.meanings))
            .unwrap_or_default()
    };

    let mut infos: Vec<PartInfo> = listed
        .iter()
        .map(|&p| {
            let mut names = meanings(&p);
            // A variant KANJIDIC can't name (亻) borrows its original's names.
            if names.is_empty() {
                names = originals.get(&p).map(meanings).unwrap_or_default();
            }
            PartInfo {
                element: p,
                names,
                strokes: by_literal
                    .get(&p)
                    .map(|e| e.stroke_count)
                    .filter(|&n| n > 0)
                    .or_else(|| variant_strokes(p))
                    .unwrap_or(0),
                kanji_count: by_part
                    .get(&p)
                    .map_or(0, |l| l.iter().filter(|&&k| k != p).count() as u32),
            }
        })
        .collect();
    // By stroke count, unknown counts last; the most used first within one.
    infos.sort_unstable_by_key(|p| {
        (
            p.strokes == 0,
            p.strokes,
            std::cmp::Reverse(p.kanji_count),
            p.element,
        )
    });

    let (kanji_parts_at, kanji_parts) = flatten(&parts_of);
    let mut parts: Vec<char> = by_part.keys().copied().collect();
    parts.sort_unstable();
    let lists: Vec<Vec<char>> = parts
        .iter()
        .map(|p| by_part.remove(p).unwrap_or_default())
        .collect();
    let (part_kanji_at, part_kanji) = flatten(&lists);

    PartIndexRecord {
        kanji,
        kanji_parts_at,
        kanji_parts,
        parts,
        part_kanji_at,
        part_kanji,
        listed: infos,
    }
}

/// Lists laid end to end, with where each one starts (plus the end).
fn flatten(lists: &[Vec<char>]) -> (Vec<u32>, Vec<char>) {
    let mut at = Vec::with_capacity(lists.len() + 1);
    let mut values = Vec::new();
    at.push(0);
    for l in lists {
        values.extend_from_slice(l);
        at.push(values.len() as u32);
    }
    (at, values)
}

/// Push `k`'s parts and theirs. KanjiVG has no cycles, but a depth cap keeps
/// a bad build from recursing forever.
fn collect_parts(k: char, entries: &HashMap<char, &KanjiEntry>, out: &mut Vec<char>, depth: u8) {
    if depth > 8 {
        return;
    }
    let Some(e) = entries.get(&k) else { return };
    for p in &e.parts {
        if p.element == k || out.contains(&p.element) {
            continue;
        }
        out.push(p.element);
        collect_parts(p.element, entries, out, depth + 1);
    }
}

/// The loaded index, ready to search.
pub(crate) struct PartIndex {
    rec: PartIndexRecord,
    /// `rec.listed`'s parts, sorted, to tell which parts the grid shows.
    listed: Vec<char>,
}

impl PartIndex {
    pub(crate) fn new(rec: PartIndexRecord) -> Self {
        let mut listed: Vec<char> = rec.listed.iter().map(|p| p.element).collect();
        listed.sort_unstable();
        Self { rec, listed }
    }

    pub(crate) fn listed(&self) -> &[PartInfo] {
        &self.rec.listed
    }

    fn parts_of(&self, k: char) -> Option<&[char]> {
        lookup(
            &self.rec.kanji,
            &self.rec.kanji_parts_at,
            &self.rec.kanji_parts,
            k,
        )
    }

    fn kanji_with(&self, p: char) -> Option<&[char]> {
        lookup(
            &self.rec.parts,
            &self.rec.part_kanji_at,
            &self.rec.part_kanji,
            p,
        )
    }

    /// The kanji holding every part of `selected` in rank order, and the
    /// listed parts those kanji hold besides (sorted).
    pub(crate) fn search(&self, selected: &[char]) -> (Vec<char>, Vec<char>) {
        let mut selected: Vec<char> = selected.to_vec();
        selected.sort_unstable();
        selected.dedup();
        // Walk the rarest part's list: the intersection can only shrink.
        let Some(lists) = selected
            .iter()
            .map(|&p| self.kanji_with(p))
            .collect::<Option<Vec<_>>>()
        else {
            return Default::default();
        };
        let Some(shortest) = lists.iter().min_by_key(|l| l.len()) else {
            return Default::default();
        };
        let matches: Vec<char> = shortest
            .iter()
            .copied()
            .filter(|&k| {
                self.parts_of(k)
                    .is_some_and(|own| selected.iter().all(|p| own.binary_search(p).is_ok()))
            })
            .collect();

        let mut open: Vec<char> = matches
            .iter()
            .filter_map(|&k| self.parts_of(k))
            .flatten()
            .copied()
            .filter(|p| self.listed.binary_search(p).is_ok() && selected.binary_search(p).is_err())
            .collect();
        open.sort_unstable();
        open.dedup();
        (matches, open)
    }
}

/// The values stored for `key` in one of the record's flat maps.
fn lookup<'a>(keys: &[char], at: &[u32], values: &'a [char], key: char) -> Option<&'a [char]> {
    let i = keys.binary_search(&key).ok()?;
    let start = *at.get(i)? as usize;
    let end = *at.get(i + 1)? as usize;
    values.get(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanjidic_types::KanjiPart;

    fn entry(
        literal: char,
        strokes: u8,
        freq: Option<u16>,
        meaning: &str,
        parts: &[(char, Option<char>)],
    ) -> KanjiEntry {
        KanjiEntry {
            literal,
            stroke_count: strokes,
            grade: None,
            freq,
            jlpt: None,
            on_readings: vec![],
            kun_readings: vec![],
            meanings: vec![meaning.to_string()],
            parts: parts
                .iter()
                .map(|&(element, original)| KanjiPart {
                    element,
                    original,
                    radical: false,
                    phonetic: false,
                })
                .collect(),
            mnemonic: None,
        }
    }

    /// 語 = 言 + 吾, 吾 = 五 + 口, 話 = 言 + 舌, 舌 = 千 + 口, 清 = 氵 + 青.
    /// 氵 has no entry of its own, like the real radical variants.
    fn sample() -> Vec<KanjiEntry> {
        vec![
            entry('語', 14, Some(300), "word", &[('言', None), ('吾', None)]),
            entry('話', 13, Some(100), "talk", &[('言', None), ('舌', None)]),
            entry('吾', 7, None, "I", &[('五', None), ('口', None)]),
            entry('舌', 6, None, "tongue", &[('千', None), ('口', None)]),
            entry(
                '清',
                11,
                Some(600),
                "pure",
                &[('氵', Some('水')), ('青', None)],
            ),
            entry('言', 7, None, "say", &[]),
            entry('口', 3, None, "mouth", &[]),
            entry('五', 4, None, "five", &[]),
            entry('千', 3, None, "thousand", &[]),
            entry('青', 8, None, "blue", &[]),
            entry('水', 4, None, "water", &[]),
        ]
    }

    fn index() -> PartIndex {
        PartIndex::new(build_index(&sample()))
    }

    #[test]
    fn parts_are_found_at_any_depth() {
        let index = index();
        // 口 sits inside 吾 and 舌, so 語 and 話 hold it too. Ranked by
        // frequency, then unranked kanji by stroke count.
        let (matches, _) = index.search(&['口']);
        assert_eq!(matches, vec!['話', '語', '口', '舌', '吾']);
        let (matches, open) = index.search(&['言', '口']);
        assert_eq!(matches, vec!['話', '語']);
        let mut expected = vec!['吾', '五', '舌', '千'];
        expected.sort_unstable();
        assert_eq!(open, expected);
    }

    #[test]
    fn unknown_or_no_parts_match_nothing() {
        let index = index();
        assert_eq!(index.search(&[]), (vec![], vec![]));
        assert_eq!(index.search(&['猫']).0, Vec::<char>::new());
        assert_eq!(index.search(&['言', '氵']).0, Vec::<char>::new());
        // Picking a part twice is picking it once.
        assert_eq!(index.search(&['氵', '氵']).0, vec!['清']);
    }

    #[test]
    fn grid_lists_only_parts_of_other_kanji() {
        let index = index();
        let listed: Vec<char> = index.listed().iter().map(|p| p.element).collect();
        // 語 and 話 are inside nothing; 水 is only named as 氵's original.
        for c in ['語', '話', '清', '水'] {
            assert!(!listed.contains(&c), "{c} listed");
        }
        let info = |c| {
            index
                .listed()
                .iter()
                .find(|p| p.element == c)
                .expect("listed")
        };
        assert_eq!((info('口').strokes, info('口').kanji_count), (3, 4));
        // Fewer strokes first: 口 and 千 (3) before 五 (4) before 言 (7).
        let pos = |c| listed.iter().position(|&x| x == c).unwrap();
        assert!(pos('口') < pos('五') && pos('五') < pos('言'));
        // 口 is in more kanji than 千, so it leads the 3-stroke row.
        assert!(pos('口') < pos('千'));
        // 氵 has no entry here: it borrows 水's name.
        assert_eq!(info('氵').names, vec!["water".to_string()]);
    }

    #[test]
    fn the_index_survives_storage() {
        let rec = build_index(&sample());
        let bytes = postcard::to_allocvec(&rec).expect("encode");
        let back: PartIndexRecord = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(back, rec);
    }
}
