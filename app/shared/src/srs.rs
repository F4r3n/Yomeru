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

    // Scale the freshly-scheduled interval by intervalScale. Stability stays as
    // FSRS computed it: the next review derives its stability from this one, so
    // scaling it here would reapply the scale on every review and compound.
    let scale = settings.interval_scale;
    let scaled = if (scale - 1.0).abs() < f64::EPSILON {
        scheduled
    } else {
        let interval_days = (scheduled.due_ms - now_ms) / MS_PER_DAY;
        srs_core::SrsCard {
            due_ms: now_ms + interval_days * scale * MS_PER_DAY,
            ..scheduled
        }
    };

    let mut next = card.clone();
    next.apply_scheduling(&scaled);
    if rating == ReviewRating::Again {
        next.again_count += 1;
    }

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

/// Minimum summed `again_count` before a word shows up as a trouble word —
/// a single slip isn't a pattern.
pub const TROUBLE_MIN_FAILS: u32 = 2;

/// How many trouble words a drill session covers. A longer list is worked
/// through batch by batch, least-drilled first — see [`pick_drill_batch`].
pub const DRILL_BATCH: usize = 10;

/// A failed card stops counting as trouble once FSRS schedules it at least
/// this far out. Every "Again" collapses the interval back to minutes, so a
/// word drops off by being remembered across real reviews and comes back as
/// soon as it's failed again. Drill answers never move the interval, so they
/// can't clear a word.
pub const TROUBLE_EXIT_INTERVAL_DAYS: f64 = 30.0;

/// A word the user keeps failing, with the cards still giving them trouble.
#[derive(Debug, Clone)]
pub struct TroubleWord {
    pub sequence: u32,
    /// `again_count` summed over the word's struggling cards, so its
    /// recognition and recall siblings rank as one word.
    pub fails: u32,
    /// The word's struggling cards — what a drill session reviews.
    pub cards: Vec<SrsCard>,
}

/// The card's current scheduled interval: the gap between its last real
/// review and its next one. `None` for a card that's never been reviewed.
fn interval_ms(c: &SrsCard) -> Option<f64> {
    c.last_review_ms.map(|last| c.due_ms - last)
}

/// A card is struggling while it's in progress (Active — graduated cards are
/// learned, staging ones never reviewed), has failed at least once, and FSRS
/// still schedules it less than `exit_interval_days` out.
fn is_struggling(c: &SrsCard, exit_interval_days: f64) -> bool {
    matches!(c.status, CardStatus::Active)
        && c.again_count > 0
        && interval_ms(c).is_some_and(|i| i < exit_interval_days * MS_PER_DAY)
}

/// Rank words by how often their struggling cards have been failed,
/// most-failed first. Ties go to the higher FSRS difficulty, then to the
/// lower sequence so the order is stable across reloads.
pub fn trouble_words(
    cards: &[SrsCard],
    min_fails: u32,
    exit_interval_days: f64,
) -> Vec<TroubleWord> {
    let mut by_seq: std::collections::BTreeMap<u32, (u32, f64, Vec<SrsCard>)> =
        std::collections::BTreeMap::new();
    for c in cards
        .iter()
        .filter(|c| is_struggling(c, exit_interval_days))
    {
        let e = by_seq.entry(c.sequence).or_insert((0, 0.0, Vec::new()));
        e.0 += c.again_count;
        e.1 = e.1.max(c.difficulty);
        e.2.push(c.clone());
    }
    let mut ranked: Vec<_> = by_seq
        .into_iter()
        .filter(|(_, (fails, _, _))| *fails >= min_fails)
        .collect();
    ranked.sort_by(|(sa, (fa, da, _)), (sb, (fb, db, _))| {
        fb.cmp(fa).then(db.total_cmp(da)).then(sa.cmp(sb))
    });
    ranked
        .into_iter()
        .map(|(sequence, (fails, _, cards))| TroubleWord {
            sequence,
            fails,
            cards,
        })
        .collect()
}

/// The next drill batch: the `batch` words drilled the fewest times, keeping
/// [`trouble_words`]' most-failed-first order among equals. Repeated drills
/// therefore rotate through the whole list instead of replaying its head.
/// `drilled` maps a word's sequence to its drill count; absent means never.
pub fn pick_drill_batch<'a>(
    words: &'a [TroubleWord],
    drilled: &std::collections::HashMap<u32, u32>,
    batch: usize,
) -> Vec<&'a TroubleWord> {
    let mut order: Vec<&TroubleWord> = words.iter().collect();
    // Stable sort: ties keep their trouble rank.
    order.sort_by_key(|w| drilled.get(&w.sequence).copied().unwrap_or(0));
    order.truncate(batch);
    order
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

    #[test]
    fn again_counts_every_failure_including_learning_steps() {
        // FSRS's `lapses` stays 0 here — the card never reached Review — but
        // these are exactly the failures the trouble list needs to see.
        let settings = settings_with_graduation(0);
        let c = fresh_card();
        let c = review_once(&c, ReviewRating::Again, 0.0, &settings);
        let c = review_once(&c, ReviewRating::Again, 60_000.0, &settings);
        let c = review_once(&c, ReviewRating::Good, 120_000.0, &settings);
        assert_eq!(c.again_count, 2);
        assert_eq!(c.lapses, 0);
    }

    #[test]
    fn non_again_ratings_leave_again_count_alone() {
        let settings = settings_with_graduation(0);
        let mut c = fresh_card();
        c.again_count = 3;
        for r in [ReviewRating::Hard, ReviewRating::Good, ReviewRating::Easy] {
            assert_eq!(review_once(&c, r, 0.0, &settings).again_count, 3);
        }
    }

    /// An Active card last reviewed at 0, next due in 10 minutes — well
    /// inside the trouble window.
    fn active(sequence: u32, direction: CardDirection, again: u32, difficulty: f64) -> SrsCard {
        let mut c = SrsCard::new(sequence, direction, 0.0);
        c.status = CardStatus::Active;
        c.again_count = again;
        c.difficulty = difficulty;
        c.last_review_ms = Some(0.0);
        c.due_ms = 10.0 * 60_000.0;
        c
    }

    fn trouble(cards: &[SrsCard]) -> Vec<TroubleWord> {
        trouble_words(cards, 2, TROUBLE_EXIT_INTERVAL_DAYS)
    }

    fn seqs(t: &[TroubleWord]) -> Vec<u32> {
        t.iter().map(|w| w.sequence).collect()
    }

    #[test]
    fn trouble_words_sum_siblings_and_rank_most_failed_first() {
        let cards = [
            active(1, CardDirection::Recognition, 3, 5.0),
            active(2, CardDirection::Recognition, 2, 5.0),
            active(2, CardDirection::Recall, 2, 5.0),
            active(3, CardDirection::Recall, 2, 5.0),
        ];
        let t = trouble(&cards);
        assert_eq!(seqs(&t), vec![2, 1, 3]);
        assert_eq!(t[0].fails, 4);
        assert_eq!(t[0].cards.len(), 2);
    }

    #[test]
    fn trouble_words_skip_graduated_staging_and_below_threshold() {
        let mut graduated = active(1, CardDirection::Recall, 9, 5.0);
        graduated.status = CardStatus::Graduated;
        let mut staging = active(2, CardDirection::Recall, 9, 5.0);
        staging.status = CardStatus::Staging;
        let cards = [
            graduated,
            staging,
            active(3, CardDirection::Recall, 1, 5.0),
            active(4, CardDirection::Recall, 2, 5.0),
        ];
        assert_eq!(seqs(&trouble(&cards)), vec![4]);
    }

    #[test]
    fn a_word_leaves_once_its_interval_reaches_the_exit_threshold() {
        let exit = TROUBLE_EXIT_INTERVAL_DAYS * MS_PER_DAY;
        let mut learned = active(1, CardDirection::Recall, 5, 5.0);
        learned.last_review_ms = Some(100.0 * MS_PER_DAY);
        learned.due_ms = 100.0 * MS_PER_DAY + exit; // exactly at the threshold
        let mut almost = active(2, CardDirection::Recall, 5, 5.0);
        almost.last_review_ms = Some(100.0 * MS_PER_DAY);
        almost.due_ms = 100.0 * MS_PER_DAY + exit - 60_000.0;
        assert_eq!(seqs(&trouble(&[learned, almost])), vec![2]);
    }

    #[test]
    fn failing_again_brings_a_word_back() {
        // Long interval → off the list; one real "Again" collapses the
        // interval to minutes and the word is trouble again.
        let settings = settings_with_graduation(0);
        let mut c = active(1, CardDirection::Recall, 2, 5.0);
        c.last_review_ms = Some(0.0);
        c.due_ms = 2.0 * TROUBLE_EXIT_INTERVAL_DAYS * MS_PER_DAY;
        assert!(trouble(std::slice::from_ref(&c)).is_empty());
        let c = review_once(&c, ReviewRating::Again, c.due_ms, &settings);
        assert_eq!(seqs(&trouble(&[c])), vec![1]);
    }

    #[test]
    fn only_struggling_siblings_count_toward_fails() {
        // Recognition failed a lot but is now learned; recall slipped once.
        // The word ranks on recall alone — 1 fail, below the threshold.
        let mut recognition = active(1, CardDirection::Recognition, 4, 5.0);
        recognition.due_ms = 2.0 * TROUBLE_EXIT_INTERVAL_DAYS * MS_PER_DAY;
        let recall = active(1, CardDirection::Recall, 1, 5.0);
        assert!(trouble(&[recognition.clone(), recall]).is_empty());
        let recall = active(1, CardDirection::Recall, 2, 5.0);
        let t = trouble(&[recognition, recall]);
        assert_eq!(t[0].fails, 2);
        assert_eq!(t[0].cards.len(), 1);
    }

    #[test]
    fn trouble_words_drill_only_the_cards_that_failed() {
        // A word whose recall keeps failing but whose recognition is fine
        // drills just the recall card.
        let cards = [
            active(1, CardDirection::Recognition, 0, 5.0),
            active(1, CardDirection::Recall, 3, 5.0),
        ];
        let t = trouble(&cards);
        assert_eq!(t[0].cards.len(), 1);
        assert_eq!(t[0].cards[0].direction, CardDirection::Recall);
    }

    #[test]
    fn trouble_words_break_ties_on_difficulty_then_sequence() {
        let cards = [
            active(5, CardDirection::Recall, 2, 4.0),
            active(7, CardDirection::Recall, 2, 8.0),
            active(6, CardDirection::Recall, 2, 4.0),
        ];
        assert_eq!(seqs(&trouble(&cards)), vec![7, 5, 6]);
    }

    fn word(sequence: u32) -> TroubleWord {
        TroubleWord {
            sequence,
            fails: 2,
            cards: vec![],
        }
    }

    fn batch_seqs(words: &[TroubleWord], drilled: &[(u32, u32)], n: usize) -> Vec<u32> {
        let drilled = drilled.iter().copied().collect();
        pick_drill_batch(words, &drilled, n)
            .iter()
            .map(|w| w.sequence)
            .collect()
    }

    #[test]
    fn drill_batch_takes_the_head_of_the_list_when_nothing_was_drilled() {
        let words: Vec<_> = (1..=12).map(word).collect();
        assert_eq!(batch_seqs(&words, &[], 10), (1..=10).collect::<Vec<_>>());
    }

    #[test]
    fn drill_batch_prefers_least_drilled_keeping_trouble_order_among_equals() {
        // 1..=10 were drilled once; 11 and 12 never — they go first, then the
        // drilled ones refill the batch in their original rank.
        let words: Vec<_> = (1..=12).map(word).collect();
        let drilled: Vec<_> = (1..=10).map(|s| (s, 1)).collect();
        assert_eq!(
            batch_seqs(&words, &drilled, 4),
            vec![11, 12, 1, 2]
        );
    }

    #[test]
    fn drill_batch_is_the_whole_list_when_shorter_than_a_batch() {
        let words: Vec<_> = (1..=3).map(word).collect();
        assert_eq!(batch_seqs(&words, &[(2, 5)], 10), vec![1, 3, 2]);
    }

    #[test]
    fn interval_scale_moves_due_date_but_not_stability() {
        // Scaling stability would compound, since FSRS builds the next review's
        // stability from it; only the due date may carry the scale.
        let plain = SrsSettings {
            graduation_interval_days: 0,
            ..Default::default()
        };
        let scaled = SrsSettings {
            interval_scale: 1.5,
            ..plain.clone()
        };
        let c = fresh_card();
        let a = review_once(&c, ReviewRating::Good, 0.0, &plain);
        let b = review_once(&c, ReviewRating::Good, 0.0, &scaled);
        assert_eq!(a.stability, b.stability);
        assert!((b.due_ms - a.due_ms * 1.5).abs() < 1.0);
    }
}
