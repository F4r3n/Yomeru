use serde::{Deserialize, Serialize};

use crate::PartOfSpeech;

/// A single JMdict dictionary entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordEntry {
    /// Sequence number from JMdict (unique entry ID).
    pub sequence: u32,
    /// Kanji (non-kana) writing forms, e.g. ["飲む", "飮む"].
    pub kanji_forms: Vec<KanjiElement>,
    /// Reading (kana) forms, e.g. ["のむ"].
    pub reading_forms: Vec<ReadingElement>,
    /// Senses (meanings), each sense can have multiple glosses and POS tags.
    pub senses: Vec<Sense>,
}

impl WordEntry {
    /// Primary headword: first kanji form, or first reading if no kanji.
    pub fn headword(&self) -> &str {
        self.kanji_forms
            .first()
            .map(|k| k.text.as_str())
            .or_else(|| self.reading_forms.first().map(|r| r.text.as_str()))
            .unwrap_or("")
    }

    /// Primary reading (first reading form).
    pub fn primary_reading(&self) -> &str {
        self.reading_forms
            .first()
            .map(|r| r.text.as_str())
            .unwrap_or("")
    }

    /// First English gloss from the first sense.
    pub fn first_gloss(&self) -> &str {
        self.senses
            .first()
            .and_then(|s| s.glosses.first())
            .map(|g| g.text.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanjiElement {
    pub text: String,
    pub info: Vec<String>,
    pub priorities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingElement {
    pub text: String,
    /// If true, this reading only applies to certain kanji forms.
    pub no_kanji: bool,
    /// Kanji forms this reading applies to (empty = all).
    pub restricted_to: Vec<String>,
    pub info: Vec<String>,
    pub priorities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sense {
    /// Part-of-speech tags (carry forward from previous sense if empty).
    pub pos: Vec<PartOfSpeech>,
    /// Glosses (translations), typically in English.
    pub glosses: Vec<Gloss>,
    /// Cross-references to other entries.
    pub xrefs: Vec<String>,
    /// Antonyms.
    pub antonyms: Vec<String>,
    /// Field of application (e.g. "math", "food").
    pub fields: Vec<String>,
    /// Miscellaneous info (e.g. "usually written in kana").
    pub misc: Vec<String>,
    /// Sense-level info notes.
    pub info: Vec<String>,
    /// Dialect tags.
    pub dialects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gloss {
    pub text: String,
    /// Language code (default "eng").
    pub lang: String,
    /// Gloss type ("lit", "fig", "expl", etc.).
    pub gloss_type: Option<String>,
}
