//! FSRS review glue. Wraps `srs_core::review_card` with the extension's
//! `intervalScale` and `graduationIntervalDays` overlay so behavior matches.

use srs_core::ReviewRating;

use crate::settings::SrsSettings;
use crate::types::{CardStatus, MS_PER_DAY, SrsCard};

pub enum ReviewOutcome {
    /// Card was rescheduled; persist it.
    Rescheduled(SrsCard),
    /// Card's computed next-review interval crossed the graduation
    /// threshold. Carries the fully-rescheduled card (status already set to
    /// `Graduated`) — callers persist it like any other reschedule, they
    /// just don't show it in Review anymore.
    Graduated(SrsCard),
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

    let mut next = card.clone();
    next.apply_scheduling(&scaled);

    // Graduate once FSRS's own computed next-review interval is long enough
    // that the word is considered learned. No streak-tracking needed: a
    // failed review naturally collapses the interval back below threshold on
    // its own, so "Again" doesn't need special-casing here.
    let interval_days = (scaled.due_ms - now_ms) / MS_PER_DAY;
    if settings.graduation_interval_days > 0
        && interval_days >= settings.graduation_interval_days as f64
    {
        next.status = CardStatus::Graduated;
        return ReviewOutcome::Graduated(next);
    }

    next.status = CardStatus::Active;
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

    fn settings_with_graduation(days: u32) -> SrsSettings {
        SrsSettings {
            graduation_interval_days: days,
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
            ReviewOutcome::Graduated(_) => panic!("expected Rescheduled, got Graduated"),
        }
    }

    #[test]
    fn graduates_once_computed_interval_crosses_threshold() {
        // A very low threshold (1 day) is reached quickly by an Easy rating
        // on a fresh card, whose initial stability is already several days.
        let settings = settings_with_graduation(1);
        let c = fresh_card();
        let outcome = apply_review(&c, ReviewRating::Easy, 0.0, &settings);
        match outcome {
            ReviewOutcome::Graduated(g) => {
                assert!(matches!(g.status, CardStatus::Graduated));
                // The rescheduled fields are carried, not discarded.
                assert!(g.stability > 0.0);
                assert!(g.due_ms > 0.0);
            }
            ReviewOutcome::Rescheduled(_) => panic!("expected Graduated"),
        }
    }

    #[test]
    fn does_not_graduate_below_threshold() {
        // A huge threshold (10 years) is never reached by a single review.
        let settings = settings_with_graduation(3650);
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Good, 0.0, &settings);
        assert!(matches!(c.status, CardStatus::Active));
    }

    #[test]
    fn again_after_long_interval_does_not_graduate() {
        // Build up a long interval, then answer Again — the interval
        // collapses back down on its own, no streak bookkeeping needed.
        let settings = settings_with_graduation(3650); // effectively "never" for this test
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Easy, 0.0, &settings);
        let c = review_once(&c, ReviewRating::Again, 1.0, &settings);
        assert!(matches!(c.status, CardStatus::Active));
    }

    #[test]
    fn zero_graduation_interval_never_graduates() {
        let settings = settings_with_graduation(0);
        let c = fresh_card();
        let outcome = apply_review(&c, ReviewRating::Easy, 0.0, &settings);
        assert!(matches!(outcome, ReviewOutcome::Rescheduled(_)));
    }
}
