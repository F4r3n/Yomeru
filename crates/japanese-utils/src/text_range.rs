/// Converts a byte offset in a UTF-8 string to a char offset.
pub fn byte_to_char_offset(text: &str, byte_offset: usize) -> Option<usize> {
    if byte_offset > text.len() {
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
