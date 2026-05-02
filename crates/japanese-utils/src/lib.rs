#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

mod char_class;
mod text_range;

pub use char_class::*;
pub use text_range::*;

/// Extracts the Japanese run starting at `char_offset` and extending forward.
/// Returns empty string if the character at `char_offset` is not Japanese.
#[cfg_attr(feature = "wasm", wasm_bindgen)]
pub fn extract_japanese_run(text: &str, char_offset: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if char_offset >= len {
        return String::new();
    }

    if !is_japanese(chars[char_offset]) {
        return String::new();
    }

    let end = (char_offset..len)
        .find(|&i| !is_japanese(chars[i]))
        .unwrap_or(len);

    chars[char_offset..end].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_run_from_mixed_text() {
        let text = "hello 飲み込む world";
        // '飲' is at char index 6
        assert_eq!(extract_japanese_run(text, 6), "飲み込む");
        // '込' is at char index 8 — returns from cursor forward
        assert_eq!(extract_japanese_run(text, 8), "込む");
    }

    #[test]
    fn returns_empty_for_non_japanese() {
        assert_eq!(extract_japanese_run("hello world", 3), "");
    }
}
