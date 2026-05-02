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
            | '\u{FF00}'..='\u{FFEF}' // Halfwidth/Fullwidth Forms
            | '\u{30FC}' // ー katakana-hiragana prolonged sound mark
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
}
