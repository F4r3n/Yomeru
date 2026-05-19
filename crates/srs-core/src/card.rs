use chrono::{DateTime, TimeZone, Utc};
use rs_fsrs::{Card as FsrsCard, State as FsrsState};
use serde::{Deserialize, Serialize};

/// FSRS learning state. Mirrors `rs_fsrs::State` but with lowercase serde names
/// so it round-trips cleanly to JSON in IDB / storage.local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CardState {
    #[default]
    New,
    Learning,
    Review,
    Relearning,
}

impl From<FsrsState> for CardState {
    fn from(s: FsrsState) -> Self {
        match s {
            FsrsState::New => Self::New,
            FsrsState::Learning => Self::Learning,
            FsrsState::Review => Self::Review,
            FsrsState::Relearning => Self::Relearning,
        }
    }
}

impl From<CardState> for FsrsState {
    fn from(s: CardState) -> Self {
        match s {
            CardState::New => Self::New,
            CardState::Learning => Self::Learning,
            CardState::Review => Self::Review,
            CardState::Relearning => Self::Relearning,
        }
    }
}

/// A single vocabulary card.
///
/// Timestamps are Unix epoch milliseconds (f64, matches JS `Date.now()`).
/// `word` / `added_ms` are bookkeeping kept across reviews; the rest is FSRS
/// scheduling state. Display data (reading, glosses) is looked up from JMdict
/// at render time and is not stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsCard {
    pub word: String,
    pub added_ms: f64,

    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: CardState,
    pub last_review_ms: Option<f64>,
}

pub(crate) fn ms_to_dt(ms: f64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms as i64)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap())
}

pub(crate) fn dt_to_ms(dt: DateTime<Utc>) -> f64 {
    dt.timestamp_millis() as f64
}

impl SrsCard {
    /// Convert the persisted shape into the `rs_fsrs::Card` the scheduler operates on.
    pub(crate) fn to_fsrs(&self) -> FsrsCard {
        FsrsCard {
            due: ms_to_dt(self.due_ms),
            stability: self.stability,
            difficulty: self.difficulty,
            elapsed_days: 0,
            scheduled_days: 0,
            reps: self.reps as i32,
            lapses: self.lapses as i32,
            state: self.state.into(),
            last_review: self
                .last_review_ms
                .map(ms_to_dt)
                .unwrap_or_else(|| ms_to_dt(self.added_ms)),
        }
    }

    /// Rebuild the persisted shape from an `rs_fsrs::Card`, preserving the bookkeeping
    /// fields (`word`, `added_ms`) which FSRS doesn't know about.
    pub(crate) fn from_fsrs(word: String, added_ms: f64, c: FsrsCard) -> Self {
        Self {
            word,
            added_ms,
            due_ms: dt_to_ms(c.due),
            stability: c.stability,
            difficulty: c.difficulty,
            reps: c.reps.max(0) as u32,
            lapses: c.lapses.max(0) as u32,
            state: c.state.into(),
            last_review_ms: Some(dt_to_ms(c.last_review)),
        }
    }
}
