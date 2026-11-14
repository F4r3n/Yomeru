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

/// Number of FSRS-5 model weights: the length of rs-fsrs's `Parameters::w`.
pub const WEIGHT_COUNT: usize = array_len(|p: Parameters| p.w);

/// `N` from a function returning `[f64; N]`, so the count follows the type.
const fn array_len<const N: usize>(_: fn(Parameters) -> [f64; N]) -> usize {
    N
}

/// The default FSRS-5 weights.
pub fn default_weights() -> [f64; WEIGHT_COUNT] {
    Parameters::default().w
}

/// `w` as a weight set, or `None` if it has the wrong length or a non-finite
/// value.
pub fn valid_weights(w: &[f64]) -> Option<[f64; WEIGHT_COUNT]> {
    <[f64; WEIGHT_COUNT]>::try_from(w)
        .ok()
        .filter(|w| w.iter().all(|x| x.is_finite()))
}

fn make_fsrs(request_retention: f64, weights: Option<&[f64]>) -> FSRS {
    // Seed is overwritten by Scheduler::init_seed from (now, reps, difficulty,
    // stability), so we leave Parameters::seed at its default and skip the
    // per-review allocation that setting it explicitly would cost.
    let mut params = Parameters {
        request_retention: request_retention.clamp(MIN_REQUEST_RETENTION, MAX_REQUEST_RETENTION),
        enable_fuzz: true,
        ..Parameters::default()
    };
    if let Some(w) = weights.and_then(valid_weights) {
        params.w = w;
    }
    FSRS::new(params)
}

/// Create a brand-new card. State = New, stability/difficulty 0, due immediately.
pub fn new_card(sequence: u32, now_ms: f64) -> SrsCard {
    SrsCard {
        sequence,
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
    review_card_with(card, rating, now_ms, request_retention, None)
}

/// Apply a review with custom model `weights` (from the optimizer). Invalid
/// or absent weights fall back to the defaults.
pub fn review_card_with(
    card: SrsCard,
    rating: ReviewRating,
    now_ms: f64,
    request_retention: f64,
    weights: Option<&[f64]>,
) -> SrsCard {
    let sequence = card.sequence;
    let added_ms = card.added_ms;
    let fsrs = make_fsrs(request_retention, weights);
    let fsrs_card: FsrsCard = card.to_fsrs();
    let info = fsrs.next(fsrs_card, ms_to_dt(now_ms), rating.to_fsrs());
    SrsCard::from_fsrs(sequence, added_ms, info.card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::CardState;

    const DAY_MS: f64 = 86_400_000.0;

    fn base_card() -> SrsCard {
        // Use any sequence; identity isn't exercised by these tests.
        new_card(1_000_001, 0.0)
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

    #[test]
    fn custom_weights_change_the_schedule_and_bad_ones_are_ignored() {
        let mut w = default_weights();
        // w[2] is the initial stability after a first "Good".
        w[2] *= 4.0;
        let stock = review_card(base_card(), ReviewRating::Good, 0.0);
        let tuned = review_card_with(base_card(), ReviewRating::Good, 0.0, 0.9, Some(&w));
        assert!(tuned.stability > stock.stability);

        let short = review_card_with(base_card(), ReviewRating::Good, 0.0, 0.9, Some(&w[..5]));
        assert_eq!(short.stability, stock.stability);
        let mut nan = w;
        nan[0] = f64::NAN;
        let bad = review_card_with(base_card(), ReviewRating::Good, 0.0, 0.9, Some(&nan));
        assert_eq!(bad.stability, stock.stability);
    }
}
