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
    let mut iter = text.char_indices();
    for _ in 0..char_offset {
        if iter.next().is_none() {
            return String::new();
        }
    }
    let (byte_start, _) = match iter.next() {
        None => return String::new(),
        Some((_, c)) if !is_japanese(c) => return String::new(),
        Some(pair) => pair,
    };
    let byte_end = iter
        .find(|(_, c)| !is_japanese(*c))
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    text[byte_start..byte_end].to_owned()
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

    #[test]
    fn returns_empty_for_out_of_bounds_offset() {
        assert_eq!(extract_japanese_run("飲む", 10), "");
        assert_eq!(extract_japanese_run("", 0), "");
    }

    #[test]
    fn all_japanese_string() {
        assert_eq!(extract_japanese_run("飲み込む", 0), "飲み込む");
    }

    #[test]
    fn japanese_at_start_of_mixed_text() {
        assert_eq!(extract_japanese_run("飲むhello", 0), "飲む");
    }

    #[test]
    fn offset_past_japanese_run_into_ascii() {
        let text = "hello飲むworld";
        // 'h','e','l','l','o' = indices 0-4, '飲' = 5, 'む' = 6, 'w' = 7
        assert_eq!(extract_japanese_run(text, 7), "");
    }
}
