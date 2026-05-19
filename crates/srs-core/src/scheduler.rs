use crate::SrsCard;

/// Filter a slice of cards to only those due at or before `now_ms`.
pub fn filter_due(cards: &[SrsCard], now_ms: f64) -> Vec<&SrsCard> {
    cards.iter().filter(|c| c.due_ms <= now_ms).collect()
}

/// Return the next due timestamp across all cards, or None if empty.
pub fn next_review_ms(cards: &[SrsCard]) -> Option<f64> {
    cards.iter().map(|c| c.due_ms).reduce(f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsrs::new_card;

    fn card_with_due(due_ms: f64) -> SrsCard {
        let mut c = new_card("word", 0.0);
        c.due_ms = due_ms;
        c
    }

    #[test]
    fn filter_due_empty() {
        assert!(filter_due(&[], 1000.0).is_empty());
    }

    #[test]
    fn filter_due_all_past() {
        let cards = vec![card_with_due(100.0), card_with_due(200.0)];
        assert_eq!(filter_due(&cards, 500.0).len(), 2);
    }

    #[test]
    fn filter_due_none_past() {
        let cards = vec![card_with_due(1000.0), card_with_due(2000.0)];
        assert!(filter_due(&cards, 500.0).is_empty());
    }

    #[test]
    fn filter_due_boundary_inclusive() {
        let cards = vec![card_with_due(500.0)];
        assert_eq!(filter_due(&cards, 500.0).len(), 1);
    }

    #[test]
    fn filter_due_mixed() {
        let cards = vec![card_with_due(100.0), card_with_due(1000.0), card_with_due(200.0)];
        let due = filter_due(&cards, 500.0);
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].due_ms, 100.0);
        assert_eq!(due[1].due_ms, 200.0);
    }

    #[test]
    fn next_review_ms_empty() {
        assert_eq!(next_review_ms(&[]), None);
    }

    #[test]
    fn next_review_ms_single() {
        let cards = vec![card_with_due(42.0)];
        assert_eq!(next_review_ms(&cards), Some(42.0));
    }

    #[test]
    fn next_review_ms_returns_minimum() {
        let cards = vec![card_with_due(300.0), card_with_due(100.0), card_with_due(200.0)];
        assert_eq!(next_review_ms(&cards), Some(100.0));
    }
}
