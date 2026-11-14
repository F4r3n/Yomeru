//! Per-user FSRS weight optimization from the review log.
//!
//! Produces FSRS-5 weights (19), the model the clients' `rs-fsrs` scheduler
//! runs. Pure apart from the training itself, so the history → training-set
//! conversion is unit-tested.

use fsrs::{FSRS, FSRSItem, FSRSReview};

/// Number of FSRS-5 weights, as fsrs defines them. Must match the clients'
/// `srs_core::WEIGHT_COUNT`.
pub const WEIGHT_COUNT: usize = fsrs::DEFAULT_PARAMETERS.len();

/// Fewer training items than this and fsrs only pretrains the first four
/// weights, which isn't worth replacing the defaults for.
pub const MIN_ITEMS: usize = 64;

const DAY_MS: i64 = 86_400_000;

/// One logged scheduled review.
#[derive(Debug, Clone, Copy)]
pub struct Answer {
    pub reviewed_ms: i64,
    /// 1 = Again … 4 = Easy.
    pub rating: u8,
}

/// A card's logged reviews plus its current `reps`, which says how many of
/// them belong to its current history.
pub struct CardHistory {
    pub answers: Vec<Answer>,
    pub reps: usize,
}

/// Training items from card histories, oldest last review first.
///
/// A card's history is its last `reps` answers: a reset zeroes `reps`, so
/// older answers belong to a previous life of the card. A card with fewer
/// answers than `reps` was reviewed before the log existed (or imported) and
/// is skipped, since its first logged answer isn't its first review.
///
/// Each answer after the first yields an item (the history up to it), kept
/// only if it came at least a day after the previous one: same-day answers
/// carry no long-term recall signal, and fsrs rejects items without one.
pub fn build_items(cards: impl IntoIterator<Item = CardHistory>) -> Vec<FSRSItem> {
    let mut items: Vec<(i64, FSRSItem)> = Vec::new();
    for mut card in cards {
        if card.reps < 2 {
            continue;
        }
        card.answers.sort_by_key(|a| a.reviewed_ms);
        let Some(start) = card.answers.len().checked_sub(card.reps) else {
            continue;
        };
        let Some(history) = card.answers.get(start..) else {
            continue;
        };
        let mut reviews = Vec::with_capacity(history.len());
        let mut prev_ms = None;
        for a in history {
            let delta_t = prev_ms.map_or(0, |p: i64| ((a.reviewed_ms - p).max(0) / DAY_MS) as u32);
            prev_ms = Some(a.reviewed_ms);
            reviews.push(FSRSReview {
                rating: u32::from(a.rating.clamp(1, 4)),
                delta_t,
            });
            if reviews.len() >= 2 && delta_t > 0 {
                items.push((
                    a.reviewed_ms,
                    FSRSItem {
                        reviews: reviews.clone(),
                    },
                ));
            }
        }
    }
    // fsrs weights recent items more and expects them in time order.
    items.sort_by_key(|(t, _)| *t);
    items.into_iter().map(|(_, item)| item).collect()
}

/// Trains weights on `items`. CPU-bound for up to a few seconds: run it off
/// the async runtime.
pub fn optimize(items: Vec<FSRSItem>) -> anyhow::Result<Vec<f64>> {
    let model = FSRS::new(None).map_err(|e| anyhow::anyhow!("fsrs init: {e:?}"))?;
    let w = model
        .compute_parameters(items, None, true)
        .map_err(|e| anyhow::anyhow!("fsrs training: {e:?}"))?;
    anyhow::ensure!(
        w.len() == WEIGHT_COUNT && w.iter().all(|x| x.is_finite()),
        "fsrs returned {} weights",
        w.len()
    );
    Ok(w.into_iter().map(f64::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i64, rating: u8) -> Answer {
        Answer {
            reviewed_ms: day * DAY_MS,
            rating,
        }
    }

    fn deltas(item: &FSRSItem) -> Vec<u32> {
        item.reviews.iter().map(|r| r.delta_t).collect()
    }

    #[test]
    fn items_are_prefixes_ending_in_a_long_term_review() {
        let items = build_items([CardHistory {
            // Two same-day answers, then 2 and 5 days later.
            answers: vec![at(0, 3), at(0, 1), at(2, 3), at(7, 4)],
            reps: 4,
        }]);
        assert_eq!(items.len(), 2);
        assert_eq!(deltas(&items[0]), vec![0, 0, 2]);
        assert_eq!(deltas(&items[1]), vec![0, 0, 2, 5]);
        assert_eq!(items[1].reviews[3].rating, 4);
    }

    #[test]
    fn only_the_last_reps_answers_count_and_incomplete_histories_are_skipped() {
        let items = build_items([
            // Reset after the first two answers: only the last two are its history.
            CardHistory {
                answers: vec![at(0, 3), at(3, 3), at(10, 3), at(12, 3)],
                reps: 2,
            },
            // Reviewed before the log existed.
            CardHistory {
                answers: vec![at(0, 3), at(5, 3)],
                reps: 5,
            },
        ]);
        assert_eq!(items.len(), 1);
        assert_eq!(deltas(&items[0]), vec![0, 2]);
    }

    #[test]
    fn items_come_out_in_time_order_across_cards() {
        let items = build_items([
            CardHistory {
                answers: vec![at(10, 3), at(20, 3)],
                reps: 2,
            },
            CardHistory {
                answers: vec![at(0, 3), at(4, 3)],
                reps: 2,
            },
        ]);
        assert_eq!(
            items.iter().map(deltas).collect::<Vec<_>>(),
            [vec![0, 4], vec![0, 10]]
        );
    }

    #[test]
    fn optimizes_a_synthetic_history_into_a_valid_weight_set() {
        // Cards answered Good on an expanding schedule, with some lapses.
        let cards = (0..200).map(|i| {
            let mut day = 0;
            let mut answers = vec![at(0, 3)];
            for step in [1, 3, 8, 20] {
                day += step;
                answers.push(at(day, if (i + step) % 7 == 0 { 1 } else { 3 }));
            }
            let reps = answers.len();
            CardHistory { answers, reps }
        });
        let items = build_items(cards);
        assert!(items.len() >= MIN_ITEMS);
        let w = optimize(items).unwrap();
        assert_eq!(w.len(), WEIGHT_COUNT);
        assert!(w.iter().all(|x| x.is_finite()));
    }
}
