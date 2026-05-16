use crate::SrsCard;

/// SM-2 rating: 0-5 where 0-2 are failures, 3-5 are successes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRating {
    /// Complete blackout — no recall at all.
    Blackout = 0,
    /// Incorrect; correct answer recognized on seeing it.
    Incorrect = 1,
    /// Incorrect; correct answer felt easy once shown.
    IncorrectEasy = 2,
    /// Correct; recalled with significant difficulty.
    Hard = 3,
    /// Correct; recalled after some hesitation.
    Good = 4,
    /// Correct; perfect recall with no hesitation.
    Perfect = 5,
}

impl ReviewRating {
    pub fn from_u8(n: u8) -> Self {
        match n {
            0 => Self::Blackout,
            1 => Self::Incorrect,
            2 => Self::IncorrectEasy,
            3 => Self::Hard,
            4 => Self::Good,
            5 => Self::Perfect,
            _ => Self::Good,
        }
    }

    pub fn score(self) -> f32 {
        self as u8 as f32
    }

    pub fn is_pass(self) -> bool {
        (self as u8) >= 3
    }
}

const MIN_EASE: f32 = 1.3;
const INITIAL_EASE: f32 = 2.5;
const MS_PER_DAY: f64 = 86_400_000.0;

/// Create a new card ready for its first review.
pub fn new_card(word: &str, now_ms: f64) -> SrsCard {
    SrsCard {
        word: word.to_owned(),
        interval_days: 0.0,
        ease_factor: INITIAL_EASE,
        repetitions: 0,
        due_ms: now_ms, // due immediately
        added_ms: now_ms,
        last_reviewed_ms: None,
    }
}

/// Apply a review rating to a card and return the updated card.
/// `now_ms` is the current timestamp in milliseconds.
pub fn review_card(mut card: SrsCard, rating: ReviewRating, now_ms: f64) -> SrsCard {
    let q = rating.score();

    // SM-2 ease factor update (applies regardless of pass/fail).
    card.ease_factor = (card.ease_factor + 0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02))
        .max(MIN_EASE);

    if rating.is_pass() {
        // Successful recall: advance interval.
        card.interval_days = match card.repetitions {
            0 => 1.0,
            1 => 6.0,
            _ => (card.interval_days * card.ease_factor).round(),
        };
        card.repetitions += 1;
    } else {
        // Failed recall: reset to beginning, due immediately.
        card.repetitions = 0;
        card.interval_days = 0.0;
    }

    card.due_ms = now_ms + card.interval_days as f64 * MS_PER_DAY;
    card.last_reviewed_ms = Some(now_ms);

    card
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_card() -> SrsCard {
        new_card("飲む", 0.0)
    }

    #[test]
    fn new_card_is_immediately_due() {
        let c = base_card();
        assert_eq!(c.due_ms, 0.0);
        assert_eq!(c.repetitions, 0);
        assert_eq!(c.ease_factor, INITIAL_EASE);
    }

    #[test]
    fn perfect_review_sequence() {
        let c = base_card();
        let c = review_card(c, ReviewRating::Perfect, 0.0);
        assert_eq!(c.repetitions, 1);
        assert_eq!(c.interval_days, 1.0);

        let c = review_card(c, ReviewRating::Perfect, MS_PER_DAY);
        assert_eq!(c.repetitions, 2);
        assert_eq!(c.interval_days, 6.0);

        let c = review_card(c, ReviewRating::Perfect, 7.0 * MS_PER_DAY);
        assert_eq!(c.repetitions, 3);
        // 6.0 * 2.6 = 15.6 → rounds to 16
        assert!(c.interval_days > 6.0);
    }

    #[test]
    fn failed_review_resets() {
        let c = base_card();
        let c = review_card(c, ReviewRating::Perfect, 0.0);
        let c = review_card(c, ReviewRating::Perfect, MS_PER_DAY);
        assert_eq!(c.repetitions, 2);

        let c = review_card(c, ReviewRating::Blackout, 7.0 * MS_PER_DAY);
        assert_eq!(c.repetitions, 0);
        assert_eq!(c.interval_days, 0.0);
        assert_eq!(c.due_ms, 7.0 * MS_PER_DAY); // due immediately
    }

    #[test]
    fn ease_factor_never_below_min() {
        let c = base_card();
        let c = review_card(c, ReviewRating::Blackout, 0.0);
        let c = review_card(c, ReviewRating::Blackout, MS_PER_DAY);
        let c = review_card(c, ReviewRating::Blackout, 2.0 * MS_PER_DAY);
        assert!(c.ease_factor >= MIN_EASE);
    }

    #[test]
    fn from_u8_all_valid_ratings() {
        assert_eq!(ReviewRating::from_u8(0), ReviewRating::Blackout);
        assert_eq!(ReviewRating::from_u8(1), ReviewRating::Incorrect);
        assert_eq!(ReviewRating::from_u8(2), ReviewRating::IncorrectEasy);
        assert_eq!(ReviewRating::from_u8(3), ReviewRating::Hard);
        assert_eq!(ReviewRating::from_u8(4), ReviewRating::Good);
        assert_eq!(ReviewRating::from_u8(5), ReviewRating::Perfect);
    }

    #[test]
    fn from_u8_out_of_range_defaults_to_good() {
        assert_eq!(ReviewRating::from_u8(6), ReviewRating::Good);
        assert_eq!(ReviewRating::from_u8(255), ReviewRating::Good);
    }

    #[test]
    fn is_pass_boundary() {
        assert!(!ReviewRating::Blackout.is_pass());
        assert!(!ReviewRating::Incorrect.is_pass());
        assert!(!ReviewRating::IncorrectEasy.is_pass());
        assert!(ReviewRating::Hard.is_pass());
        assert!(ReviewRating::Good.is_pass());
        assert!(ReviewRating::Perfect.is_pass());
    }

    #[test]
    fn last_reviewed_ms_updated() {
        let c = base_card();
        let c = review_card(c, ReviewRating::Good, 12345.0);
        assert_eq!(c.last_reviewed_ms, Some(12345.0));
    }

    #[test]
    fn hard_rating_advances_interval() {
        let c = base_card();
        let c = review_card(c, ReviewRating::Hard, 0.0);
        assert_eq!(c.repetitions, 1);
        assert_eq!(c.interval_days, 1.0);
        assert!(c.ease_factor < INITIAL_EASE);
    }
}
