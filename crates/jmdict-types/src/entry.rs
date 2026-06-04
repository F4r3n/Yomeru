use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::PartOfSpeech;

/// A single JMdict dictionary entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
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

impl ArchivedWordEntry {
    /// Primary headword: first kanji form, or first reading if no kanji.
    /// Mirror of [`WordEntry::headword`] over the zero-copy archived layout.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct KanjiElement {
    pub text: String,
    /// `ke_inf` tags — e.g. "rK" (rarely used kanji form), "sK" (search-only),
    /// "iK" (irregular), "oK" (out-dated).
    pub info: Vec<String>,
    /// `ke_pri` frequency tags — e.g. "news1", "ichi1", "spec1", "gai1",
    /// "nf01"–"nf48".
    pub priorities: Vec<String>,
}

impl KanjiElement {
    pub fn from_text(content: impl Into<String>) -> Self {
        Self {
            text: content.into(),
            info: vec![],
            priorities: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
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
    /// `re_pri` frequency tags — same vocabulary as `KanjiElement.priorities`.
    pub priorities: Vec<String>,
}

impl ReadingElement {
    pub fn from_reading(reading: impl Into<String>) -> Self {
        Self {
            text: reading.into(),
            #[cfg(feature = "full")]
            no_kanji: false,
            #[cfg(feature = "full")]
            restricted_to: vec![],
            #[cfg(feature = "full")]
            info: vec![],
            priorities: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Archive, RkyvSerialize, RkyvDeserialize)]
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
    /// Miscellaneous tags — most usefully "uk" (usually written in kana).
    pub misc: Vec<String>,
    #[cfg(feature = "full")]
    /// Sense-level info notes.
    pub info: Vec<String>,
    #[cfg(feature = "full")]
    /// Dialect tags.
    pub dialects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Gloss {
    pub text: String,
    /// Language code (default "eng").
    pub lang: String,
    /// Gloss type ("lit", "fig", "expl", etc.).
    pub gloss_type: Option<String>,
}

impl Gloss {
    pub fn new(
        content: impl Into<String>,
        lang: impl Into<String>,
        gloss_type: Option<String>,
    ) -> Self {
        Self {
            text: content.into(),
            lang: lang.into(),
            gloss_type,
        }
    }
}
