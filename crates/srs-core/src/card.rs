use serde::{Deserialize, Serialize};

/// A single vocabulary card in the SRS system.
/// All timestamps are Unix epoch milliseconds (f64 to match JS Date.now()).
/// Display data (reading, meaning, senses) is derived from JMdict at render time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsCard {
    /// Japanese headword (kanji or kana) — the dictionary key.
    pub word: String,
    /// SM-2: current review interval in days.
    pub interval_days: f32,
    /// SM-2: ease factor (starts at 2.5, min 1.3).
    pub ease_factor: f32,
    /// SM-2: number of successful repetitions in sequence.
    pub repetitions: u32,
    /// Timestamp (ms) when this card is next due for review.
    pub due_ms: f64,
    /// Timestamp (ms) when this card was first added.
    pub added_ms: f64,
    /// Timestamp (ms) of the most recent review, if any.
    pub last_reviewed_ms: Option<f64>,
}
