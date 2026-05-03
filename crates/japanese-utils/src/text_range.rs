/// Converts a byte offset in a UTF-8 string to a char offset.
pub fn byte_to_char_offset(text: &str, byte_offset: usize) -> Option<usize> {
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        return None;
    }
    Some(text[..byte_offset].chars().count())
}

/// Converts a char offset to a byte offset.
pub fn char_to_byte_offset(text: &str, char_offset: usize) -> Option<usize> {
    text.char_indices()
        .nth(char_offset)
        .map(|(byte_idx, _)| byte_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_to_char_ascii() {
        let s = "hello";
        assert_eq!(byte_to_char_offset(s, 0), Some(0));
        assert_eq!(byte_to_char_offset(s, 3), Some(3));
        assert_eq!(byte_to_char_offset(s, 5), Some(5));
    }

    #[test]
    fn byte_to_char_multibyte() {
        // "飲む" — 飲 is 3 bytes, む is 3 bytes
        let s = "飲む";
        assert_eq!(byte_to_char_offset(s, 0), Some(0));
        assert_eq!(byte_to_char_offset(s, 3), Some(1));
        assert_eq!(byte_to_char_offset(s, 6), Some(2));
    }

    #[test]
    fn byte_to_char_out_of_bounds() {
        let s = "hi";
        assert_eq!(byte_to_char_offset(s, 3), None);
    }

    #[test]
    fn byte_to_char_mid_codepoint_returns_none() {
        // 飲 is 3 bytes; offsets 1 and 2 are not char boundaries
        assert_eq!(byte_to_char_offset("飲む", 1), None);
        assert_eq!(byte_to_char_offset("飲む", 2), None);
    }

    #[test]
    fn char_to_byte_ascii() {
        let s = "hello";
        assert_eq!(char_to_byte_offset(s, 0), Some(0));
        assert_eq!(char_to_byte_offset(s, 4), Some(4));
    }

    #[test]
    fn char_to_byte_multibyte() {
        // "飲む" — 飲 is at byte 0, む is at byte 3
        let s = "飲む";
        assert_eq!(char_to_byte_offset(s, 0), Some(0));
        assert_eq!(char_to_byte_offset(s, 1), Some(3));
    }

    #[test]
    fn char_to_byte_out_of_bounds() {
        let s = "hi";
        assert_eq!(char_to_byte_offset(s, 5), None);
    }

    #[test]
    fn roundtrip_byte_char() {
        let s = "abc飲む漢字xyz";
        for (char_idx, (byte_idx, _)) in s.char_indices().enumerate() {
            assert_eq!(byte_to_char_offset(s, byte_idx), Some(char_idx));
            assert_eq!(char_to_byte_offset(s, char_idx), Some(byte_idx));
        }
    }
}
