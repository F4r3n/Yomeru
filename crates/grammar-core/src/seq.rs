//! The `sequence` a grammar point's review card has, until cards key on
//! `kind` + `item` (`docs/phase-4-grammar-cards.md`, "Card keys").
//!
//! Cards, card stores and sync key on a `u32` `sequence`: a JMdict `ent_seq`
//! for words (1,000,000 to below 10,000,000), a code point for kanji (below
//! 0x40000). A grammar card's is the entry's `seq` with the top bit set, so
//! the three ranges never meet.

/// The top bit every grammar card's `sequence` has.
pub const GRAMMAR_SEQ_BIT: u32 = 0x8000_0000;

/// The card `sequence` of the entry numbered `seq`.
pub fn card_sequence(seq: u32) -> u32 {
    GRAMMAR_SEQ_BIT | seq
}

/// Whether `sequence` belongs to a grammar card.
pub fn is_grammar_seq(sequence: u32) -> bool {
    sequence & GRAMMAR_SEQ_BIT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_cards_sit_above_words_and_kanji() {
        assert_eq!(card_sequence(417), 0x8000_01A1);
        assert!(is_grammar_seq(card_sequence(1)));
        assert!(!is_grammar_seq(2_900_000));
        assert!(!is_grammar_seq(u32::from('漢')));
    }
}
