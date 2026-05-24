//! FSRS review glue. Wraps `srs_core::review_card` with the extension's
//! `intervalScale` and `graduationReps` overlay so behavior matches.

use srs_core::ReviewRating;

use crate::settings::SrsSettings;
use crate::types::{CardStatus, SrsCard, MS_PER_DAY};

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

    if settings.graduation_reps > 0 && scaled.reps >= settings.graduation_reps {
        return ReviewOutcome::Graduated;
    }

    let mut next = card.clone();
    next.apply_scheduling(&scaled);
    next.status = CardStatus::Active;
    ReviewOutcome::Rescheduled(next)
}

pub fn now_ms() -> f64 {
    js_sys::Date::now()
}
