use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KanjiEntry {
    pub literal: char,
    pub stroke_count: u8,
    pub grade: Option<u8>,
    pub freq: Option<u16>,
    pub jlpt: Option<u8>,
    pub on_readings: Vec<String>,
    pub kun_readings: Vec<String>,
    pub meanings: Vec<String>,
    /// Top-level components from KanjiVG, in writing order. Empty for kanji
    /// KanjiVG doesn't cover or doesn't split. Defaulted so JSON from a server
    /// that predates parts still parses.
    #[serde(default)]
    pub parts: Vec<KanjiPart>,
    /// A short phrase linking the parts to the meaning, from the separate
    /// mnemonic file (`kanji-mnemonics.tsv`). Never stored in `kanjidic.bin`:
    /// it's laid over the entry at lookup time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mnemonic: Option<String>,
}

/// How an entry is laid out in `kanjidic.bin` (version 2). Kept apart from
/// [`KanjiEntry`] so the entry can grow fields that come from elsewhere
/// without changing the binary format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KanjiRecord {
    pub literal: char,
    pub stroke_count: u8,
    pub grade: Option<u8>,
    pub freq: Option<u16>,
    pub jlpt: Option<u8>,
    pub on_readings: Vec<String>,
    pub kun_readings: Vec<String>,
    pub meanings: Vec<String>,
    pub parts: Vec<KanjiPart>,
}

impl From<KanjiRecord> for KanjiEntry {
    fn from(r: KanjiRecord) -> Self {
        KanjiEntry {
            literal: r.literal,
            stroke_count: r.stroke_count,
            grade: r.grade,
            freq: r.freq,
            jlpt: r.jlpt,
            on_readings: r.on_readings,
            kun_readings: r.kun_readings,
            meanings: r.meanings,
            parts: r.parts,
            mnemonic: None,
        }
    }
}

impl From<&KanjiEntry> for KanjiRecord {
    fn from(e: &KanjiEntry) -> Self {
        KanjiRecord {
            literal: e.literal,
            stroke_count: e.stroke_count,
            grade: e.grade,
            freq: e.freq,
            jlpt: e.jlpt,
            on_readings: e.on_readings.clone(),
            kun_readings: e.kun_readings.clone(),
            meanings: e.meanings.clone(),
            parts: e.parts.clone(),
        }
    }
}

/// One component of a kanji, e.g. 言 and 吾 in 語. Its own components come
/// from looking up `element` in turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KanjiPart {
    pub element: char,
    /// The full form a variant stands for, e.g. 水 for 氵: where its meaning
    /// comes from when the variant has no entry of its own.
    pub original: Option<char>,
    /// The part the kanji is classified under in dictionaries.
    pub radical: bool,
    /// The part that hints at the kanji's on reading.
    pub phonetic: bool,
}

/// A part as the kanji finder's grid lists it: something seen inside other
/// kanji.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartInfo {
    pub element: char,
    /// 0 when KANJIDIC has no entry for the part (⺍).
    pub strokes: u8,
    /// How many kanji contain it, at any depth.
    pub kanji_count: u32,
}

/// The kanji finder's index, computed by `kanjidic-build` and stored at the
/// end of `kanjidic.bin` (version 3), so loading it decodes a few flat lists
/// rather than every entry.
///
/// Both maps are laid out flat: the values for `keys[i]` are
/// `values[at[i]..at[i + 1]]`, and `at` has one more element than `keys`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartIndexRecord {
    /// Every kanji, sorted.
    pub kanji: Vec<char>,
    pub kanji_parts_at: Vec<u32>,
    /// Each kanji's parts at any depth, itself included, sorted.
    pub kanji_parts: Vec<char>,
    /// Every part, each kanji's own literal included, sorted.
    pub parts: Vec<char>,
    pub part_kanji_at: Vec<u32>,
    /// Each part's kanji, best known first.
    pub part_kanji: Vec<char>,
    /// The parts some other kanji contains, in grid order.
    pub listed: Vec<PartInfo>,
}

/// The kanji containing every selected part, best known first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartSearch {
    /// The first matches, cut at the caller's limit.
    pub kanji: Vec<KanjiEntry>,
    /// Every match, before the cut.
    pub total: u32,
    /// Parts some match contains besides the selected ones: picking one of
    /// these still leaves a result. Every other part can be greyed out.
    pub open_parts: Vec<char>,
}

/// `meanings` as labels for a part chip: KANJIDIC's radical bookkeeping is
/// trimmed ("dotted cliff radical (no. 53)" → "dotted cliff") or dropped when
/// that leaves nothing ("radical number 9"), and duplicates go. Empty means
/// the caller should fall back to the part's original form (人 for 亻).
pub fn part_meanings(meanings: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in meanings.iter().filter_map(|m| clean_part_meaning(m)) {
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

fn clean_part_meaning(m: &str) -> Option<String> {
    let m = m.trim();
    for prefix in ["radical number ", "variant of radical "] {
        if let Some(n) = m.strip_prefix(prefix) {
            if n.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
        }
    }
    // KANJIDIC sometimes leaves the bracket open: "clothing radical (no. 145".
    let base = match m.rfind("(no.") {
        Some(i) => m[..i].trim_end(),
        None => m,
    };
    let base = base.strip_suffix(" variant").unwrap_or(base);
    let base = base.strip_suffix(" radical").unwrap_or(base).trim();
    (!base.is_empty() && base != "radical").then(|| base.to_string())
}

#[cfg(test)]
mod tests {
    use super::part_meanings;

    fn pm(ms: &[&str]) -> Vec<String> {
        part_meanings(&ms.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn bare_radical_labels_are_dropped() {
        assert!(pm(&["radical number 9"]).is_empty());
        assert!(pm(&["variant of radical 146"]).is_empty());
    }

    #[test]
    fn descriptive_radical_labels_are_trimmed_and_deduped() {
        assert_eq!(pm(&["dotted cliff radical (no. 53)"]), ["dotted cliff"]);
        assert_eq!(pm(&["net radical variant (no. 122)"]), ["net"]);
        assert_eq!(
            pm(&["heart", "mind", "heart radical (no. 61)"]),
            ["heart", "mind"]
        );
        assert_eq!(pm(&["one", "one radical (no.1)"]), ["one"]);
        assert_eq!(pm(&["clothing", "clothing radical (no. 145"]), ["clothing"]);
    }

    #[test]
    fn ordinary_meanings_are_kept() {
        assert_eq!(pm(&["some", "or", "possibly"]), ["some", "or", "possibly"]);
    }
}
