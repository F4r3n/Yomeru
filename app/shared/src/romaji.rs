//! Romaji → hiragana conversion, shared by anywhere the app lets a user type
//! Japanese input in ASCII (Lookup, and the search bars in New Words / Word
//! List). Mirrors `extension/src/options/romaji.ts`.

pub fn is_romaji(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
}

/// True if a word (given its headword and reading) matches a user's search
/// `query` — substring match against the headword, plus a substring match
/// against the reading using the query as typed and, if it looks like
/// romaji, its hiragana conversion. `query` must already be trimmed and
/// lowercased by the caller (`headword`/`reading` are lowercased/compared
/// here). An empty query always matches.
pub fn matches_query(headword: &str, reading: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    if headword.to_lowercase().contains(query) || reading.contains(query) {
        return true;
    }
    is_romaji(query) && reading.contains(&romaji_to_hiragana(query))
}

/// Hepburn romaji → hiragana. Mirrors `extension/src/options/romaji.ts`.
pub fn romaji_to_hiragana(input: &str) -> String {
    let s = input.to_lowercase();
    let mut out = String::with_capacity(s.len()); // Pre-allocate to avoid re-allocations
    let mut remainder = s.as_str();

    while !remainder.is_empty() {
        // 1. Handle double consonants (Sokuon 'っ')
        let mut chars = remainder.chars();
        if let (Some(c1), Some(c2)) = (chars.next(), chars.next())
            && c1 != 'n'
            && c1.is_ascii_alphabetic()
            && !matches!(c1, 'a' | 'e' | 'i' | 'o' | 'u')
            && c1 == c2
        {
            out.push('っ');
            remainder = &remainder[c1.len_utf8()..];
            continue;
        }

        // 2. Handle standalone 'n' (ん)
        if remainder.starts_with('n') {
            let next_char = remainder.chars().nth(1);
            if next_char.is_none() || !matches!(next_char, Some('a' | 'e' | 'i' | 'o' | 'u' | 'y'))
            {
                out.push('ん');
                remainder = &remainder[1..];
                continue;
            }
        }

        // 3. Match Romaji chunks (3, 2, or 1 chars) without allocating Strings
        let mut matched = false;
        // Check chunks by character count, mapping them to byte lengths
        for char_len in [3, 2, 1] {
            let byte_end = remainder
                .char_indices()
                .nth(char_len)
                .map_or(remainder.len(), |(idx, _)| idx);
            let chunk = &remainder[..byte_end];

            if !chunk.is_empty()
                && let Some(rep) = lookup_romaji(chunk)
            {
                out.push_str(rep);
                remainder = &remainder[byte_end..];
                matched = true;
                break;
            }

            // If we've reached the end of the string, no need to try smaller lengths
            if byte_end == remainder.len() && char_len > remainder.chars().count() {
                continue;
            }
        }

        // 4. Fallback for un-matched characters (punctuation, spaces, etc.)
        if !matched && let Some(c) = remainder.chars().next() {
            out.push(c);
            remainder = &remainder[c.len_utf8()..];
        }
    }

    out
}

fn lookup_romaji(c: &str) -> Option<&'static str> {
    Some(match c {
        "kya" => "きゃ",
        "kyu" => "きゅ",
        "kyo" => "きょ",
        "sha" => "しゃ",
        "shu" => "しゅ",
        "sho" => "しょ",
        "shi" => "し",
        "cha" => "ちゃ",
        "chu" => "ちゅ",
        "cho" => "ちょ",
        "chi" => "ち",
        "tsu" => "つ",
        "nya" => "にゃ",
        "nyu" => "にゅ",
        "nyo" => "にょ",
        "hya" => "ひゃ",
        "hyu" => "ひゅ",
        "hyo" => "ひょ",
        "mya" => "みゃ",
        "myu" => "みゅ",
        "myo" => "みょ",
        "rya" => "りゃ",
        "ryu" => "りゅ",
        "ryo" => "りょ",
        "gya" => "ぎゃ",
        "gyu" => "ぎゅ",
        "gyo" => "ぎょ",
        "ja" => "じゃ",
        "ju" => "じゅ",
        "jo" => "じょ",
        "ji" => "じ",
        "jya" => "じゃ",
        "jyu" => "じゅ",
        "jyo" => "じょ",
        "bya" => "びゃ",
        "byu" => "びゅ",
        "byo" => "びょ",
        "pya" => "ぴゃ",
        "pyu" => "ぴゅ",
        "pyo" => "ぴょ",
        "ka" => "か",
        "ki" => "き",
        "ku" => "く",
        "ke" => "け",
        "ko" => "こ",
        "ga" => "が",
        "gi" => "ぎ",
        "gu" => "ぐ",
        "ge" => "げ",
        "go" => "ご",
        "sa" => "さ",
        "su" => "す",
        "se" => "せ",
        "so" => "そ",
        "za" => "ざ",
        "zu" => "ず",
        "ze" => "ぜ",
        "zo" => "ぞ",
        "ta" => "た",
        "te" => "て",
        "to" => "と",
        "da" => "だ",
        "de" => "で",
        "do" => "ど",
        "na" => "な",
        "ni" => "に",
        "nu" => "ぬ",
        "ne" => "ね",
        "no" => "の",
        "ha" => "は",
        "hi" => "ひ",
        "fu" => "ふ",
        "he" => "へ",
        "ho" => "ほ",
        "ba" => "ば",
        "bi" => "び",
        "bu" => "ぶ",
        "be" => "べ",
        "bo" => "ぼ",
        "pa" => "ぱ",
        "pi" => "ぴ",
        "pu" => "ぷ",
        "pe" => "ぺ",
        "po" => "ぽ",
        "ma" => "ま",
        "mi" => "み",
        "mu" => "む",
        "me" => "め",
        "mo" => "も",
        "ya" => "や",
        "yu" => "ゆ",
        "yo" => "よ",
        "ra" => "ら",
        "ri" => "り",
        "ru" => "る",
        "re" => "れ",
        "ro" => "ろ",
        "wa" => "わ",
        "wo" => "を",
        "a" => "あ",
        "i" => "い",
        "u" => "う",
        "e" => "え",
        "o" => "お",
        "n" => "ん",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(romaji_to_hiragana(""), "");
    }

    #[test]
    fn single_vowels_map() {
        assert_eq!(romaji_to_hiragana("a"), "あ");
        assert_eq!(romaji_to_hiragana("i"), "い");
        assert_eq!(romaji_to_hiragana("u"), "う");
        assert_eq!(romaji_to_hiragana("e"), "え");
        assert_eq!(romaji_to_hiragana("o"), "お");
    }

    #[test]
    fn basic_cv_syllables() {
        assert_eq!(romaji_to_hiragana("ka"), "か");
        assert_eq!(romaji_to_hiragana("su"), "す");
        assert_eq!(romaji_to_hiragana("to"), "と");
        assert_eq!(romaji_to_hiragana("ne"), "ね");
        assert_eq!(romaji_to_hiragana("mo"), "も");
    }

    #[test]
    fn prefers_longest_match() {
        // "kya" must consume all three chars, not split into "k" + "ya".
        assert_eq!(romaji_to_hiragana("kya"), "きゃ");
        assert_eq!(romaji_to_hiragana("ryu"), "りゅ");
    }

    #[test]
    fn three_char_digraphs() {
        assert_eq!(romaji_to_hiragana("sha"), "しゃ");
        assert_eq!(romaji_to_hiragana("cho"), "ちょ");
        assert_eq!(romaji_to_hiragana("tsu"), "つ");
    }

    #[test]
    fn sokuon_doubles_consonant() {
        // Doubled non-vowel, non-n consonant → っ + the syllable.
        assert_eq!(romaji_to_hiragana("kka"), "っか");
        assert_eq!(romaji_to_hiragana("tta"), "った");
        assert_eq!(romaji_to_hiragana("sshi"), "っし");
        assert_eq!(romaji_to_hiragana("kitte"), "きって");
    }

    #[test]
    fn sokuon_skips_n_and_vowels() {
        // "nn" must become ん + n…, NOT っ — the algorithm excludes 'n' from
        // the gemination rule because "nn" is the standard way to type ん
        // before a vowel.
        assert_eq!(romaji_to_hiragana("nni"), "んに");
        // Doubled vowels are just two vowels, never っ.
        assert_eq!(romaji_to_hiragana("aa"), "ああ");
        assert_eq!(romaji_to_hiragana("oo"), "おお");
    }

    #[test]
    fn n_before_consonant_becomes_hiragana_n() {
        assert_eq!(romaji_to_hiragana("nko"), "んこ");
        assert_eq!(romaji_to_hiragana("konnichiwa"), "こんにちわ");
    }

    #[test]
    fn n_before_vowel_or_y_stays_attached() {
        // "na" is the syllable な, NOT ん + あ.
        assert_eq!(romaji_to_hiragana("na"), "な");
        assert_eq!(romaji_to_hiragana("ni"), "に");
        // "nya" is the digraph にゃ, NOT ん + や.
        assert_eq!(romaji_to_hiragana("nya"), "にゃ");
        assert_eq!(romaji_to_hiragana("nyu"), "にゅ");
    }

    #[test]
    fn bare_and_terminal_n() {
        assert_eq!(romaji_to_hiragana("n"), "ん");
        assert_eq!(romaji_to_hiragana("san"), "さん");
        assert_eq!(romaji_to_hiragana("ramen"), "らめん");
    }

    #[test]
    fn ji_variants_both_map_to_zi() {
        assert_eq!(romaji_to_hiragana("ji"), "じ");
        // Both nihon-shiki ("jya") and hepburn-ish ("ja") map to じゃ.
        assert_eq!(romaji_to_hiragana("ja"), "じゃ");
        assert_eq!(romaji_to_hiragana("jya"), "じゃ");
    }

    #[test]
    fn input_is_lowercased() {
        assert_eq!(romaji_to_hiragana("Ka"), "か");
        assert_eq!(romaji_to_hiragana("KONNICHIWA"), "こんにちわ");
    }

    #[test]
    fn unmatched_chars_pass_through() {
        // Hyphens, punctuation, spaces aren't in the table — preserved verbatim.
        assert_eq!(romaji_to_hiragana("ka-ki"), "か-き");
        assert_eq!(romaji_to_hiragana("ka ki"), "か き");
        assert_eq!(romaji_to_hiragana("ka, ki"), "か, き");
    }

    #[test]
    fn matches_query_empty_query_matches_everything() {
        assert!(matches_query("猫", "ねこ", ""));
    }

    #[test]
    fn matches_query_romaji_matches_via_reading() {
        assert!(matches_query("猫", "ねこ", "neko"));
        assert!(!matches_query("猫", "ねこ", "inu"));
    }

    #[test]
    fn matches_query_plain_kana_matches_reading_directly() {
        assert!(matches_query("猫", "ねこ", "ねこ"));
    }

    #[test]
    fn matches_query_headword_substring_is_case_insensitive() {
        assert!(matches_query("Neko", "ねこ", "nek"));
    }
}
