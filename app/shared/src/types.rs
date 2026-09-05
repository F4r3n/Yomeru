use serde::{Deserialize, Serialize};
use srs_core::CardState;

pub const MS_PER_DAY: f64 = 86_400_000.0;

/// Schema version of the card export/import format. Bump whenever the on-disk
/// [`SrsCard`] shape changes. Exports without this field (or with a lower value)
/// are the legacy v1 format ([`SrsCardV1`]), keyed by headword rather than
/// JMdict `sequence`.
pub const CARDS_SCHEMA_VERSION: u64 = 2;

/// Ceiling on [`SrsCard::priority`] — re-clicking "Add" on an already-staged
/// word bumps it, but stops moving the needle past this.
pub const MAX_PRIORITY: u32 = 10;

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
    /// FSRS's computed next-review interval reached the graduation threshold
    /// (`SrsSettings::graduation_interval_days`) — the card is considered
    /// learned. Kept (not deleted) and excluded from Review, shown in Word
    /// List, and reversible via [`crate::idb::reset_card`].
    Graduated,
}

/// A card as persisted in IndexedDB. Wraps the FSRS scheduling fields with
/// identity (`id`, `direction`) and lifecycle (`status`) metadata.
///
/// `sequence` is the JMdict `ent_seq` of the dictionary entry the card was
/// added from. The display headword/reading are looked up on demand and not
/// stored here, so renaming an entry in JMdict propagates automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsCard {
    pub id: String,
    pub sequence: u32,
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
    #[serde(default)]
    pub priority: u32,
    /// Wall-clock ms of the last local write, and the sync merge key. Stamped
    /// by [`crate::idb::put_cards`] so every mutation advances it — including
    /// the ones `last_review_ms` misses (a Staging→Active promotion, a priority
    /// bump) or actively moves backwards ([`SrsCard::reset_progression`], which
    /// clears the review time). Defaulted for cards stored before the field
    /// existed; read those through [`SrsCard::version_ms`].
    #[serde(default)]
    pub updated_ms: f64,
}

pub fn card_id(sequence: u32, direction: CardDirection) -> String {
    format!("{sequence}::{}", direction.as_str())
}

/// Card shape used by exports that predate the `version` field. These cards were
/// keyed by the headword string (`word`) — the `id` was `{word}::{direction}` —
/// rather than the JMdict `sequence`. Imported by resolving `word` to its
/// sequence via the dictionary, then [`upgrade`](SrsCardV1::upgrade)-ing.
#[derive(Debug, Clone, Deserialize)]
pub struct SrsCardV1 {
    pub word: String,
    pub direction: CardDirection,
    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: CardState,
    #[serde(default)]
    pub last_review_ms: Option<f64>,
    pub added_ms: f64,
    pub status: CardStatus,
}

impl SrsCardV1 {
    /// Upgrade to the current [`SrsCard`], re-keying by the resolved JMdict
    /// `sequence` (the legacy `word`-based `id` is discarded).
    pub fn upgrade(self, sequence: u32) -> SrsCard {
        SrsCard {
            id: card_id(sequence, self.direction),
            sequence,
            direction: self.direction,
            due_ms: self.due_ms,
            stability: self.stability,
            difficulty: self.difficulty,
            reps: self.reps,
            lapses: self.lapses,
            state: self.state,
            last_review_ms: self.last_review_ms,
            added_ms: self.added_ms,
            status: self.status,
            priority: 0,
            updated_ms: self.last_review_ms.unwrap_or(0.0).max(self.added_ms),
        }
    }
}

impl SrsCard {
    /// Build a new staging card from FSRS-fresh scheduling fields.
    pub fn new(sequence: u32, direction: CardDirection, now_ms: f64) -> Self {
        Self {
            id: card_id(sequence, direction),
            sequence,
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
            priority: 0,
            updated_ms: now_ms,
        }
    }

    /// The card's merge version. Falls back to the review/added times for cards
    /// written before `updated_ms` existed, so the first sync after upgrading
    /// compares something meaningful instead of an all-zero tie. Both inputs are
    /// already agreed on by every device, so the fallback is deterministic.
    pub fn version_ms(&self) -> f64 {
        if self.updated_ms > 0.0 {
            self.updated_ms
        } else {
            self.last_review_ms.unwrap_or(0.0).max(self.added_ms)
        }
    }

    pub fn to_scheduling(&self) -> srs_core::SrsCard {
        srs_core::SrsCard {
            sequence: self.sequence,
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

    /// Resets FSRS progress back to a fresh "New" card while preserving
    /// identity/status/`added_ms`/`priority`. Destructive; callers must
    /// confirm with the user first.
    pub fn reset_progression(&mut self, now_ms: f64) {
        self.due_ms = now_ms;
        self.stability = 0.0;
        self.difficulty = 0.0;
        self.reps = 0;
        self.lapses = 0;
        self.state = CardState::New;
        self.last_review_ms = None;
    }
}
