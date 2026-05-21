use serde::{Deserialize, Serialize};
use srs_core::CardState;

pub const MS_PER_DAY: f64 = 86_400_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardDirection {
    Recognition,
    Recall,
}

impl CardDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recognition => "recognition",
            Self::Recall => "recall",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardStatus {
    Staging,
    Active,
}

/// A card as persisted in IndexedDB. Wraps the FSRS scheduling fields with
/// identity (`id`, `direction`) and lifecycle (`status`) metadata.
///
/// Mirrors the extension's `SrsCard` in `src/shared/types.ts` so the same JSON
/// can be exchanged with the server and the extension's backups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsCard {
    pub id: String,
    pub word: String,
    pub direction: CardDirection,
    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: CardState,
    pub last_review_ms: Option<f64>,
    pub added_ms: f64,
    pub status: CardStatus,
}

pub fn card_id(word: &str, direction: CardDirection) -> String {
    format!("{word}::{}", direction.as_str())
}

impl SrsCard {
    /// Build a new staging card from FSRS-fresh scheduling fields.
    pub fn new(word: &str, direction: CardDirection, now_ms: f64) -> Self {
        Self {
            id: card_id(word, direction),
            word: word.to_owned(),
            direction,
            due_ms: now_ms,
            stability: 0.0,
            difficulty: 0.0,
            reps: 0,
            lapses: 0,
            state: CardState::New,
            last_review_ms: None,
            added_ms: now_ms,
            status: CardStatus::Staging,
        }
    }

    pub fn to_scheduling(&self) -> srs_core::SrsCard {
        srs_core::SrsCard {
            word: self.word.clone(),
            added_ms: self.added_ms,
            due_ms: self.due_ms,
            stability: self.stability,
            difficulty: self.difficulty,
            reps: self.reps,
            lapses: self.lapses,
            state: self.state,
            last_review_ms: self.last_review_ms,
        }
    }

    pub fn apply_scheduling(&mut self, s: &srs_core::SrsCard) {
        self.due_ms = s.due_ms;
        self.stability = s.stability;
        self.difficulty = s.difficulty;
        self.reps = s.reps;
        self.lapses = s.lapses;
        self.state = s.state;
        self.last_review_ms = s.last_review_ms;
    }
}
