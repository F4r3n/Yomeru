//! FSRS review glue. Wraps `srs_core::review_card` with the extension's
//! `intervalScale` and `graduationReps` overlay so behavior matches.

use srs_core::ReviewRating;

use crate::settings::SrsSettings;
use crate::types::{CardStatus, MS_PER_DAY, SrsCard};

pub enum ReviewOutcome {
    /// Card was rescheduled; persist it.
    Rescheduled(SrsCard),
    /// Card hit the graduation threshold; delete it.
    Graduated,
}

pub fn rating_from_u8(n: u8) -> ReviewRating {
    ReviewRating::from_u8(n)
}

pub fn apply_review(
    card: &SrsCard,
    rating: ReviewRating,
    now_ms: f64,
    settings: &SrsSettings,
) -> ReviewOutcome {
    let scheduled = srs_core::review_card_with_retention(
        card.to_scheduling(),
        rating,
        now_ms,
        settings.request_retention,
    );

    // Scale the freshly-scheduled interval (stability + due_ms) by intervalScale.
    let scale = settings.interval_scale;
    let scaled = if (scale - 1.0).abs() < f64::EPSILON {
        scheduled
    } else {
        let interval_days = (scheduled.due_ms - now_ms) / MS_PER_DAY;
        srs_core::SrsCard {
            stability: scheduled.stability * scale,
            due_ms: now_ms + interval_days * scale * MS_PER_DAY,
            ..scheduled
        }
    };

    // Consecutive correct (non-Again) reviews in a row. Graduation counts a
    // streak of successes, not FSRS's cumulative `reps` — an "Again" resets
    // it to 0 rather than merely pausing it.
    let streak = if rating == ReviewRating::Again {
        0
    } else {
        card.consecutive_correct + 1
    };

    if settings.graduation_reps > 0 && streak >= settings.graduation_reps {
        return ReviewOutcome::Graduated;
    }

    let mut next = card.clone();
    next.apply_scheduling(&scaled);
    next.status = CardStatus::Active;
    next.consecutive_correct = streak;
    ReviewOutcome::Rescheduled(next)
}

pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SrsSettings;
    use crate::types::CardDirection;

    fn settings_with_graduation(n: u32) -> SrsSettings {
        SrsSettings {
            graduation_reps: n,
            ..Default::default()
        }
    }

    fn fresh_card() -> SrsCard {
        SrsCard::new(1_000_001, CardDirection::Recall, 0.0)
    }

    /// Reviews `card` once and unwraps the `Rescheduled` outcome, panicking
    /// (test failure) if it graduated instead — helper for building up a
    /// multi-review sequence in tests where graduation is not expected yet.
    fn review_once(
        card: &SrsCard,
        rating: ReviewRating,
        now_ms: f64,
        settings: &SrsSettings,
    ) -> SrsCard {
        match apply_review(card, rating, now_ms, settings) {
            ReviewOutcome::Rescheduled(c) => c,
            ReviewOutcome::Graduated => panic!("expected Rescheduled, got Graduated"),
        }
    }

    #[test]
    fn again_resets_streak_to_zero() {
        let settings = settings_with_graduation(3);
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Good, 0.0, &settings);
        assert_eq!(c.consecutive_correct, 1);
        let c = review_once(&c, ReviewRating::Again, 1.0, &settings);
        assert_eq!(c.consecutive_correct, 0);
    }

    #[test]
    fn consecutive_passes_accumulate_and_graduate_at_threshold() {
        let settings = settings_with_graduation(3);
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Good, 0.0, &settings);
        let c = review_once(&c, ReviewRating::Good, 1.0, &settings);
        assert_eq!(c.consecutive_correct, 2);
        let outcome = apply_review(&c, ReviewRating::Good, 2.0, &settings);
        assert!(matches!(outcome, ReviewOutcome::Graduated));
    }

    #[test]
    fn interrupted_streak_does_not_graduate_even_though_cumulative_reps_would_have() {
        // Good, Again, Good, Good: 4 total reviews (old cumulative-reps
        // behavior would graduate at graduation_reps=4), but the streak is
        // only 2 — must not graduate.
        let settings = settings_with_graduation(4);
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Good, 0.0, &settings);
        let c = review_once(&c, ReviewRating::Again, 1.0, &settings);
        let c = review_once(&c, ReviewRating::Good, 2.0, &settings);
        let c = review_once(&c, ReviewRating::Good, 3.0, &settings);
        assert_eq!(c.consecutive_correct, 2);
        assert!(c.reps >= 4);
    }

    #[test]
    fn zero_graduation_reps_never_graduates() {
        let settings = settings_with_graduation(0);
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Good, 0.0, &settings);
        let c = review_once(&c, ReviewRating::Good, 1.0, &settings);
        let outcome = apply_review(&c, ReviewRating::Good, 2.0, &settings);
        assert!(matches!(outcome, ReviewOutcome::Rescheduled(_)));
    }
}
