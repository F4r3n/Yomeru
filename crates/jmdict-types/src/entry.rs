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
    #[cfg(feature = "full")]
    pub info: Vec<String>,
    #[cfg(feature = "full")]
    pub priorities: Vec<String>,
}

impl KanjiElement {
    pub fn from_text(content: String) -> Self {
        Self {
            text: content,
            #[cfg(feature = "full")]
            info: vec![],
            #[cfg(feature = "full")]
            priorities: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingElement {
    pub text: String,
    #[cfg(feature = "full")]
    /// If true, this reading only applies to certain kanji forms.
    pub no_kanji: bool,
    #[cfg(feature = "full")]
    /// Kanji forms this reading applies to (empty = all).
    pub restricted_to: Vec<String>,
    #[cfg(feature = "full")]
    pub info: Vec<String>,
    #[cfg(feature = "full")]
    pub priorities: Vec<String>,
}

impl ReadingElement {
    pub fn from_reading(reading: String) -> Self {
        Self {
            text: reading,
            #[cfg(feature = "full")]
            no_kanji: false,
            #[cfg(feature = "full")]
            restricted_to: vec![],
            #[cfg(feature = "full")]
            info: vec![],
            #[cfg(feature = "full")]
            priorities: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Sense {
    /// Part-of-speech tags (carry forward from previous sense if empty).
    pub pos: Vec<PartOfSpeech>,
    /// English glosses (translations).
    pub glosses: Vec<Gloss>,
    #[cfg(feature = "full")]
    /// Cross-references to other entries.
    pub xrefs: Vec<String>,
    #[cfg(feature = "full")]
    /// Antonyms.
    pub antonyms: Vec<String>,
    #[cfg(feature = "full")]
    /// Field of application (e.g. "math", "food").
    pub fields: Vec<String>,
    #[cfg(feature = "full")]
    /// Miscellaneous info (e.g. "usually written in kana").
    pub misc: Vec<String>,
    #[cfg(feature = "full")]
    /// Sense-level info notes.
    pub info: Vec<String>,
    #[cfg(feature = "full")]
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

impl Gloss {
    pub fn new(content: String, lang: String, gloss_type: Option<String>) -> Self {
        Self {
            text: content,
            lang,
            gloss_type,
        }
    }
}
