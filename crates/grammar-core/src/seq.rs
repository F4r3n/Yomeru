//! The number a grammar point's review card is keyed by.
//!
//! Cards, card stores and sync key on a `u32` `sequence`: a JMdict `ent_seq`
//! for words (1,000,000 to below 10,000,000), a code point for kanji (below
//! 0x40000). A grammar card's `sequence` is a hash of the point's id with the
//! top bit set, so the three ranges never meet.
//!
//! **Never change this function**: cards on every device and on the server
//! carry its values. `compile` refuses two ids with the same number.

/// The top bit every grammar card's `sequence` has.
pub const GRAMMAR_SEQ_BIT: u32 = 0x8000_0000;

/// The `sequence` of the review card for grammar point `id`: 32-bit FNV-1a
/// of the id's bytes, top bit set.
pub fn grammar_seq(id: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    GRAMMAR_SEQ_BIT | (h & !GRAMMAR_SEQ_BIT)
}

/// Whether `sequence` belongs to a grammar card.
pub fn is_grammar_seq(sequence: u32) -> bool {
    sequence & GRAMMAR_SEQ_BIT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the values: a change here re-keys every grammar card.
    #[test]
    fn values_are_fixed() {
        // FNV-1a 32 of "" is 0x811c9dc5, of "a" 0xe40c292c.
        assert_eq!(grammar_seq(""), 0x811c_9dc5);
        assert_eq!(grammar_seq("a"), 0xe40c_292c);
        assert_eq!(grammar_seq("te-wa-ikenai"), grammar_seq("te-wa-ikenai"));
        assert!(is_grammar_seq(grammar_seq("te-mo-ii")));
        assert!(!is_grammar_seq(2_900_000));
        assert!(!is_grammar_seq(u32::from('漢')));
    }
}
