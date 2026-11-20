//! Word costs from IPADIC (MeCab's dictionary, `mecab-ipadic-2.7.0`), put
//! on JMdict's spellings as [`WordCost`]s for the segmenter.
//!
//! IPADIC lists ~390k words, each with a cost MeCab learned from a tagged
//! corpus: the lower, the likelier. A cost only compares with the costs of
//! its own part of speech (MeCab balances each against its connection
//! costs: a する noun like 貿易 sits near -2000, a plain noun like 拠点
//! near 5000), so each is taken against the median of the words sharing
//! its left context id. Conjugated rows and names (people, organisations)
//! are left out: JMdict spells its words in their dictionary form and has
//! no names.
//!
//! The input is the release tarball, gunzipped (`mecab-ipadic.tar`): its
//! CSVs are EUC-JP.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jmdict_types::{PartOfSpeech, WordCost, WordEntry};

/// The parts of speech whose costs the segmenter can take (IPADIC's pos1):
/// a word's cost only ranks the JMdict entries of the same one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

impl Kind {
    fn of_pos1(pos1: &str) -> Option<Self> {
        match pos1 {
            "名詞" => Some(Self::Noun),
            "動詞" => Some(Self::Verb),
            "形容詞" => Some(Self::Adjective),
            "副詞" => Some(Self::Adverb),
            _ => None,
        }
    }

    /// Whether a JMdict part of speech is this one: 名詞 takes nouns,
    /// pronouns, な and の adjectives, numbers, counters and suffixes too.
    fn fits(self, p: &PartOfSpeech) -> bool {
        use PartOfSpeech as P;
        match self {
            Self::Noun => matches!(
                p,
                P::Noun
                    | P::NounAdverbial
                    | P::NounProper
                    | P::NounSuffix
                    | P::NounPrefix
                    | P::NounTemporal
                    | P::Pronoun
                    | P::AdjectiveNa
                    | P::AdjectiveNo
                    | P::Numeric
                    | P::Counter
                    | P::Suffix
                    | P::VerbSuru
            ),
            Self::Verb => format!("{p:?}").starts_with("Verb"),
            Self::Adjective => matches!(p, P::Adjective | P::AdjectiveFormal),
            // 今ごろ, 後ほど: adverbs to IPADIC, nouns too to JMdict.
            Self::Adverb => matches!(
                p,
                P::Adverb | P::AdverbTo | P::NounAdverbial | P::NounTemporal
            ),
        }
    }
}

/// Spelling → `(reading in hiragana, part of speech, cost)` of each IPADIC
/// noun, verb and adjective.
pub struct Costs(HashMap<String, Vec<(String, Kind, WordCost)>>);

/// The files of a tar archive, as `(name, bytes)`.
fn tar_files(bytes: &[u8]) -> Result<Vec<(String, &[u8])>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(header) = bytes.get(at..at + 512) {
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let field = |r: std::ops::Range<usize>| {
            let f = &header[r];
            let end = f.iter().position(|&b| b == 0).unwrap_or(f.len());
            String::from_utf8_lossy(&f[..end]).trim().to_owned()
        };
        let name = field(0..100);
        let size = usize::from_str_radix(&field(124..136), 8)
            .with_context(|| format!("bad tar size for {name}"))?;
        let kind = header[156];
        let data = bytes
            .get(at + 512..at + 512 + size)
            .with_context(|| format!("tar cut off in {name}"))?;
        if kind == b'0' || kind == 0 {
            out.push((name, data));
        }
        at += 512 + size.div_ceil(512) * 512;
    }
    Ok(out)
}

fn hiragana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

pub fn load(path: &Path) -> Result<Costs> {
    let bytes = std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?;
    // (spelling, reading, part of speech, left id, cost) of every row kept.
    let mut rows: Vec<(String, String, Kind, u32, i32)> = Vec::new();
    for (name, data) in tar_files(&bytes)? {
        if !name.ends_with(".csv") {
            continue;
        }
        let (text, _, bad) = encoding_rs::EUC_JP.decode(data);
        if bad {
            bail!("{name} isn't EUC-JP");
        }
        for line in text.lines() {
            let f: Vec<&str> = line.split(',').collect();
            let [
                surface,
                left,
                _right,
                cost,
                pos1,
                _pos2,
                pos3,
                _pos4,
                _ctype,
                cform,
                _base,
                reading,
                ..,
            ] = f[..]
            else {
                continue;
            };
            if !matches!(cform, "*" | "基本形") || matches!(pos3, "人名" | "組織") {
                continue;
            }
            let Some(kind) = Kind::of_pos1(pos1) else {
                continue;
            };
            let (Ok(left), Ok(cost)) = (left.parse(), cost.parse()) else {
                continue;
            };
            rows.push((surface.to_owned(), hiragana(reading), kind, left, cost));
        }
    }
    if rows.is_empty() {
        bail!("no IPADIC words in {}", path.display());
    }
    let mut by_id: HashMap<u32, Vec<i32>> = HashMap::new();
    for (_, _, _, id, cost) in &rows {
        by_id.entry(*id).or_default().push(*cost);
    }
    let medians: HashMap<u32, i32> = by_id
        .into_iter()
        .map(|(id, mut costs)| {
            costs.sort_unstable();
            (id, costs[costs.len() / 2])
        })
        .collect();
    let mut map: HashMap<String, Vec<(String, Kind, WordCost)>> = HashMap::new();
    for (surface, reading, kind, id, cost) in rows {
        let relative = (cost - medians[&id]).clamp(i16::MIN.into(), i16::MAX.into());
        map.entry(surface)
            .or_default()
            .push((reading, kind, relative as WordCost));
    }
    Ok(Costs(map))
}

impl Costs {
    /// The commonest IPADIC word spelled `form`, read as one of
    /// `readings` (in hiragana), of a part of speech `e` has. None
    /// otherwise: another word's cost (the particle は's for 歯 read は)
    /// would rank this one.
    fn of(&self, e: &WordEntry, form: &str, readings: &[String]) -> Option<WordCost> {
        let pos: Vec<&PartOfSpeech> = e.senses.iter().flat_map(|s| &s.pos).collect();
        self.0
            .get(form)?
            .iter()
            .filter(|(r, kind, _)| readings.contains(r) && pos.iter().any(|p| kind.fits(p)))
            .map(|&(_, _, c)| c)
            .min()
    }

    /// Puts a cost on every spelling of `entries` IPADIC has; returns how
    /// many spellings got one, and how many there are.
    pub fn apply(&self, entries: &mut [WordEntry]) -> (usize, usize) {
        let (mut found, mut all) = (0, 0);
        for e in entries {
            let readings: Vec<(String, &jmdict_types::ReadingElement)> = e
                .reading_forms
                .iter()
                .map(|r| (hiragana(&r.text), r))
                .collect();
            let mut kanji_costs = Vec::with_capacity(e.kanji_forms.len());
            for k in &e.kanji_forms {
                // The readings this spelling has.
                let own: Vec<String> = readings
                    .iter()
                    .filter(|(_, r)| reads(r, &k.text))
                    .map(|(h, _)| h.clone())
                    .collect();
                kanji_costs.push(self.of(e, &k.text, &own));
            }
            for (k, cost) in e.kanji_forms.iter_mut().zip(kanji_costs) {
                all += 1;
                found += usize::from(cost.is_some());
                k.word_cost = cost;
            }
            let reading_costs: Vec<Option<WordCost>> = e
                .reading_forms
                .iter()
                .map(|r| self.of(e, &r.text, &[hiragana(&r.text)]))
                .collect();
            for (r, cost) in e.reading_forms.iter_mut().zip(reading_costs) {
                all += 1;
                found += usize::from(cost.is_some());
                r.word_cost = cost;
            }
        }
        (found, all)
    }
}

/// Whether `r` is a reading of the kanji spelling `kanji`.
#[cfg(feature = "full")]
fn reads(r: &jmdict_types::ReadingElement, kanji: &str) -> bool {
    !r.no_kanji && (r.restricted_to.is_empty() || r.restricted_to.iter().any(|k| k == kanji))
}

#[cfg(not(feature = "full"))]
fn reads(_: &jmdict_types::ReadingElement, _: &str) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katakana_reads_as_hiragana() {
        assert_eq!(hiragana("キョテン"), "きょてん");
        assert_eq!(hiragana("ヨーサイ"), "よーさい");
    }

    #[test]
    fn a_tar_lists_its_files() {
        let mut tar = vec![0u8; 512];
        tar[..5].copy_from_slice(b"a.csv");
        tar[124..135].copy_from_slice(b"00000000003");
        tar[156] = b'0';
        tar.extend_from_slice(b"abc");
        tar.resize(1024 + 1024, 0);
        let files = tar_files(&tar).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "a.csv");
        assert_eq!(files[0].1, b"abc");
    }
}
