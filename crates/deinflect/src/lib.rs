mod rules;

use std::collections::VecDeque;
use rules::RULES;

/// Result of deinflecting a word.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Deinflected {
    /// Candidate dictionary form (e.g. "食べる").
    pub text: String,
    /// Human-readable description of how it was transformed (e.g. "negative past").
    pub reason: String,
}

/// Return every plausible dictionary form of `text` by repeatedly applying
/// deinflection rules. The original form is always included first.
/// Results are deduplicated and ordered from least to most transformed.
pub fn deinflect(text: &str) -> Vec<Deinflected> {
    // (candidate, reason, depth)
    let mut queue: VecDeque<(String, String, u8)> = VecDeque::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut results: Vec<Deinflected> = Vec::new();

    queue.push_back((text.to_owned(), String::new(), 0));

    while let Some((candidate, reason, depth)) = queue.pop_front() {
        if !seen.insert(candidate.clone()) {
            continue;
        }

        results.push(Deinflected { text: candidate.clone(), reason: reason.clone() });

        if depth >= 3 {
            continue;
        }

        for rule in RULES {
            let Some(stem) = candidate.strip_suffix(rule.suffix_in) else { continue };

            // Don't produce single-kana candidates from multi-char input
            // (avoids garbage like "" or "い" matching half a word).
            let new_text = format!("{}{}", stem, rule.suffix_out);
            let char_count = new_text.chars().count();
            if char_count < 1 || (text.chars().count() > 2 && char_count < 2) {
                continue;
            }

            if seen.contains(&new_text) {
                continue;
            }

            let new_reason = if reason.is_empty() {
                rule.reason.to_owned()
            } else {
                format!("{} < {}", reason, rule.reason)
            };

            queue.push_back((new_text, new_reason, depth + 1));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(text: &str) -> Vec<String> {
        deinflect(text).into_iter().map(|d| d.text).collect()
    }

    #[test]
    fn ichidan_negative() {
        assert!(forms("食べない").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_past() {
        assert!(forms("食べた").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_te_form() {
        assert!(forms("食べて").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_passive() {
        assert!(forms("食べられる").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_negative_past() {
        assert!(forms("食べなかった").contains(&"食べる".to_owned()));
    }

    #[test]
    fn ichidan_chained_passive_negative() {
        // 食べられなかった → 食べられる → 食べる  (two steps)
        assert!(forms("食べられなかった").contains(&"食べる".to_owned()));
    }

    #[test]
    fn godan_ku_negative() {
        assert!(forms("書かない").contains(&"書く".to_owned()));
    }

    #[test]
    fn godan_ku_past() {
        assert!(forms("書いた").contains(&"書く".to_owned()));
    }

    #[test]
    fn godan_mu_progressive() {
        assert!(forms("飲んでいる").contains(&"飲む".to_owned()));
    }

    #[test]
    fn godan_ru_polite() {
        assert!(forms("帰ります").contains(&"帰る".to_owned()));
    }

    #[test]
    fn adjective_negative() {
        assert!(forms("高くない").contains(&"高い".to_owned()));
    }

    #[test]
    fn adjective_past() {
        assert!(forms("高かった").contains(&"高い".to_owned()));
    }

    #[test]
    fn suru_verb() {
        assert!(forms("勉強した").contains(&"勉強する".to_owned()));
    }

    #[test]
    fn kuru_verb() {
        assert!(forms("来なかった").contains(&"来る".to_owned()));
    }
}
