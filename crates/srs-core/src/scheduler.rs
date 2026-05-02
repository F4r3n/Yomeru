use crate::SrsCard;

/// Filter a slice of cards to only those due at or before `now_ms`.
pub fn filter_due(cards: &[SrsCard], now_ms: f64) -> Vec<&SrsCard> {
    cards.iter().filter(|c| c.due_ms <= now_ms).collect()
}

/// Return the next due timestamp across all cards, or None if empty.
pub fn next_review_ms(cards: &[SrsCard]) -> Option<f64> {
    cards.iter().map(|c| c.due_ms).reduce(f64::min)
}
