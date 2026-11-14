use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExampleEntry {
    pub japanese: String,
    pub english: String,
    /// `(start, len)` in chars of the looked-up word as written in `japanese`.
    #[serde(default)]
    pub mark: Option<(u32, u32)>,
}

/// One example of a headword in the index: where its sentence sits in the
/// sentences blob and where the word appears in it (`mark_len == 0`: unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExampleRef {
    pub offset: u32,
    pub mark_start: u16,
    pub mark_len: u16,
}

/// A sentence as stored in the blob: `(japanese, english)`.
pub type StoredSentence = (String, String);
