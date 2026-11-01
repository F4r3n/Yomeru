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
