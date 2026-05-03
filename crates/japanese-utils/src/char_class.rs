pub fn is_kanji(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        | '\u{20000}'..='\u{2A6DF}' // CJK Extension B
        | '\u{2A700}'..='\u{2B73F}' // CJK Extension C
        | '\u{2B740}'..='\u{2B81F}' // CJK Extension D
    )
}

pub fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{309F}')
}

pub fn is_katakana(c: char) -> bool {
    matches!(c, '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}')
}

pub fn is_kana(c: char) -> bool {
    is_hiragana(c) || is_katakana(c)
}

/// Returns true for characters that typically appear in Japanese text:
/// kanji, kana, Japanese punctuation, and prolonged sound marks.
pub fn is_japanese(c: char) -> bool {
    is_kanji(c)
        || is_kana(c)
        || matches!(c,
            '\u{3000}'..='\u{303F}' // CJK Symbols and Punctuation
            | '\u{FF01}'..='\u{FF9F}' // Fullwidth Latin + Halfwidth Katakana
            | '\u{FFE0}'..='\u{FFEF}' // Fullwidth/Halfwidth signs (skips Halfwidth Hangul U+FFA0-FFDC)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_correctly() {
        assert!(is_kanji('飲'));
        assert!(is_hiragana('み'));
        assert!(is_katakana('ミ'));
        assert!(!is_japanese('a'));
        assert!(!is_japanese(' '));
    }

    #[test]
    fn kanji_range_boundaries() {
        assert!(is_kanji('\u{4E00}'));
        assert!(is_kanji('\u{9FFF}'));
        assert!(!is_kanji('\u{4DFF}'));
        assert!(!is_kanji('\u{A000}'));
    }

    #[test]
    fn kanji_extension_a() {
        assert!(is_kanji('\u{3400}'));
        assert!(is_kanji('\u{4DBF}'));
    }

    #[test]
    fn hiragana_range_boundaries() {
        assert!(is_hiragana('\u{3041}'));
        assert!(is_hiragana('\u{309F}'));
        assert!(!is_hiragana('\u{3040}'));
        assert!(!is_hiragana('\u{30A0}'));
    }

    #[test]
    fn katakana_range_boundaries() {
        assert!(is_katakana('\u{30A0}'));
        assert!(is_katakana('\u{30FF}'));
        assert!(!is_katakana('\u{309F}'));
        assert!(!is_katakana('\u{3100}'));
    }

    #[test]
    fn katakana_phonetic_extensions() {
        assert!(is_katakana('\u{31F0}'));
        assert!(is_katakana('\u{31FF}'));
    }

    #[test]
    fn is_kana_delegates() {
        assert!(is_kana('あ'));
        assert!(is_kana('ア'));
        assert!(!is_kana('a'));
        assert!(!is_kana('漢'));
    }

    #[test]
    fn is_japanese_japanese_punctuation() {
        assert!(is_japanese('\u{3000}'));
        assert!(is_japanese('\u{303F}'));
        assert!(is_japanese('。'));
        assert!(is_japanese('、'));
    }

    #[test]
    fn is_japanese_prolonged_sound_mark() {
        assert!(is_japanese('ー'));
    }

    #[test]
    fn is_japanese_fullwidth_forms() {
        assert!(is_japanese('\u{FF01}')); // first assigned fullwidth form
        assert!(is_japanese('\u{FF9F}')); // last halfwidth katakana
        assert!(is_japanese('\u{FFE0}')); // first fullwidth sign
        assert!(is_japanese('\u{FFEF}'));
        assert!(is_japanese('Ａ')); // U+FF21 fullwidth Latin A
    }

    #[test]
    fn is_japanese_rejects_halfwidth_hangul() {
        assert!(!is_japanese('\u{FFA0}'));
        assert!(!is_japanese('\u{FFDC}'));
    }

    #[test]
    fn is_japanese_rejects_ascii_and_latin() {
        for c in 'a'..='z' {
            assert!(!is_japanese(c));
        }
        assert!(!is_japanese('é'));
        assert!(!is_japanese('ñ'));
    }
}
