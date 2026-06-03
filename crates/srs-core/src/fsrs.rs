use crate::card::{ms_to_dt, SrsCard};
use rs_fsrs::{Card as FsrsCard, Parameters, Rating, FSRS};

/// FSRS rating: 4-grade Again/Hard/Good/Easy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl ReviewRating {
    pub fn from_u8(n: u8) -> Self {
        match n {
            1 => Self::Again,
            2 => Self::Hard,
            3 => Self::Good,
            4 => Self::Easy,
            _ => Self::Good,
        }
    }

    fn to_fsrs(self) -> Rating {
        match self {
            Self::Again => Rating::Again,
            Self::Hard => Rating::Hard,
            Self::Good => Rating::Good,
            Self::Easy => Rating::Easy,
        }
    }
}

/// Default target retention. Matches Anki/FSRS reference default.
pub const DEFAULT_REQUEST_RETENTION: f64 = 0.9;

/// Practical bounds for desired retention. Outside this band FSRS schedules
/// either runaway-long (→1.0) or punishingly short (→0.0) intervals, so any
/// caller-supplied value is clamped here before it reaches the scheduler.
pub const MIN_REQUEST_RETENTION: f64 = 0.70;
pub const MAX_REQUEST_RETENTION: f64 = 0.97;

fn make_fsrs(request_retention: f64) -> FSRS {
    // Seed is overwritten by Scheduler::init_seed from (now, reps, difficulty,
    // stability), so we leave Parameters::seed at its default and skip the
    // per-review allocation that setting it explicitly would cost.
    let params = Parameters {
        request_retention: request_retention.clamp(MIN_REQUEST_RETENTION, MAX_REQUEST_RETENTION),
        enable_fuzz: true,
        ..Parameters::default()
    };
    FSRS::new(params)
}

/// Create a brand-new card. State = New, stability/difficulty 0, due immediately.
pub fn new_card(word: &str, now_ms: f64) -> SrsCard {
    SrsCard {
        word: word.to_owned(),
        added_ms: now_ms,
        due_ms: now_ms,
        stability: 0.0,
        difficulty: 0.0,
        reps: 0,
        lapses: 0,
        state: crate::card::CardState::New,
        last_review_ms: None,
    }
}

/// Apply a review to a card at the default 0.9 target retention. Returns the
/// updated card with FSRS-scheduled `due_ms`.
pub fn review_card(card: SrsCard, rating: ReviewRating, now_ms: f64) -> SrsCard {
    review_card_with_retention(card, rating, now_ms, DEFAULT_REQUEST_RETENTION)
}

/// Apply a review at a caller-chosen target retention. `request_retention` is
/// clamped to `[MIN_REQUEST_RETENTION, MAX_REQUEST_RETENTION]`.
pub fn review_card_with_retention(
    card: SrsCard,
    rating: ReviewRating,
    now_ms: f64,
    request_retention: f64,
) -> SrsCard {
    let word = card.word.clone();
    let added_ms = card.added_ms;
    let fsrs = make_fsrs(request_retention);
    let fsrs_card: FsrsCard = card.to_fsrs();
    let info = fsrs.next(fsrs_card, ms_to_dt(now_ms), rating.to_fsrs());
    SrsCard::from_fsrs(word, added_ms, info.card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::CardState;

    const DAY_MS: f64 = 86_400_000.0;

    fn base_card() -> SrsCard {
        new_card("飲む", 0.0)
    }

    #[test]
    fn new_card_is_immediately_due_and_new_state() {
        let c = base_card();
        assert_eq!(c.due_ms, 0.0);
        assert_eq!(c.reps, 0);
        assert_eq!(c.lapses, 0);
        assert_eq!(c.state, CardState::New);
        assert_eq!(c.stability, 0.0);
    }

    #[test]
    fn good_review_advances_state_and_stability() {
        let c = base_card();
        let c = review_card(c, ReviewRating::Good, 0.0);
        assert!(c.reps >= 1);
        assert!(c.stability > 0.0);
        assert!(c.difficulty > 0.0);
        assert!(c.due_ms > 0.0);
        assert_ne!(c.state, CardState::New);
    }

    #[test]
    fn again_after_mature_keeps_partial_stability() {
        let mut c = base_card();
        c = review_card(c, ReviewRating::Good, 0.0);
        c = review_card(c, ReviewRating::Good, DAY_MS);
        c = review_card(c, ReviewRating::Good, 7.0 * DAY_MS);
        let stab_before = c.stability;

        let c = review_card(c, ReviewRating::Again, 20.0 * DAY_MS);
        assert_eq!(c.lapses, 1);
        // FSRS keeps partial stability rather than zeroing — the whole point of
        // moving off SM-2.
        assert!(c.stability > 0.0);
        assert!(c.stability < stab_before);
    }

    #[test]
    fn easy_yields_at_least_good_interval() {
        let now = 0.0;
        let good = review_card(base_card(), ReviewRating::Good, now);
        let easy = review_card(base_card(), ReviewRating::Easy, now);
        assert!(easy.stability >= good.stability);
    }

    #[test]
    fn hard_yields_at_most_good_interval() {
        let now = 0.0;
        let hard = review_card(base_card(), ReviewRating::Hard, now);
        let good = review_card(base_card(), ReviewRating::Good, now);
        assert!(hard.stability <= good.stability);
    }

    #[test]
    fn rating_from_u8_known_values() {
        assert_eq!(ReviewRating::from_u8(1), ReviewRating::Again);
        assert_eq!(ReviewRating::from_u8(2), ReviewRating::Hard);
        assert_eq!(ReviewRating::from_u8(3), ReviewRating::Good);
        assert_eq!(ReviewRating::from_u8(4), ReviewRating::Easy);
    }

    #[test]
    fn rating_from_u8_unknown_defaults_to_good() {
        assert_eq!(ReviewRating::from_u8(0), ReviewRating::Good);
        assert_eq!(ReviewRating::from_u8(99), ReviewRating::Good);
    }

    #[test]
    fn last_review_ms_set_after_review() {
        let c = review_card(base_card(), ReviewRating::Good, 12345.0);
        assert_eq!(c.last_review_ms, Some(12345.0));
    }

    #[test]
    fn default_retention_matches_plain_review_card() {
        // The fuzz seed is derived from (now, reps, difficulty, stability),
        // identical for both calls, so the results must match exactly.
        let a = review_card(base_card(), ReviewRating::Good, 0.0);
        let b = review_card_with_retention(
            base_card(),
            ReviewRating::Good,
            0.0,
            DEFAULT_REQUEST_RETENTION,
        );
        assert_eq!(a.stability, b.stability);
        assert_eq!(a.due_ms, b.due_ms);
    }

    #[test]
    fn lower_retention_schedules_further_out() {
        // Stability after the first Good review is independent of retention;
        // the resulting interval is not — a lower target pushes due_ms later.
        let strict = review_card_with_retention(base_card(), ReviewRating::Good, 0.0, 0.95);
        let relaxed = review_card_with_retention(base_card(), ReviewRating::Good, 0.0, 0.80);
        assert!(relaxed.due_ms >= strict.due_ms);
    }

    #[test]
    fn retention_is_clamped_to_safe_range() {
        // An absurd value must not panic and must behave like the clamp bound.
        let absurd = review_card_with_retention(base_card(), ReviewRating::Good, 0.0, 0.01);
        let at_min =
            review_card_with_retention(base_card(), ReviewRating::Good, 0.0, MIN_REQUEST_RETENTION);
        assert_eq!(absurd.due_ms, at_min.due_ms);
    }
}
