//! The vocabulary hint of a production card.

use super::{Example, Span, plain};

/// One word of a production card's vocabulary hint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WordHint {
    /// The dictionary form, as the sentence writes it (kana for kana).
    pub word: String,
    /// Its reading; empty when it is the word itself.
    pub reading: String,
    pub gloss: String,
}

/// The vocabulary hint of a production card: the words of its reference
/// `example` (`words`, from [`crate::platform::DictClient::sentence_words`])
/// minus the point's own, which would give the answer away, and minus
/// particles, the copula and short kana words, which the user knows and
/// which only crowd the list. Each word once.
pub fn hint_words(words: &[crate::platform::SentenceWord], example: &Example) -> Vec<WordHint> {
    use jmdict_types::PartOfSpeech as P;
    let mut point = Vec::new();
    for span in &example.ja {
        let on = matches!(span, Span::Highlight { .. });
        point.extend(std::iter::repeat_n(on, span.text().chars().count()));
    }
    let sentence: Vec<char> = plain(&example.ja).chars().collect();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for w in words {
        let in_point = (w.start..w.start + w.len).all(|k| point.get(k).copied().unwrap_or(false));
        let surface: String = sentence.iter().skip(w.start).take(w.len).collect();
        let function_word = w.entry.senses.first().is_some_and(|s| {
            s.pos.iter().any(|p| {
                matches!(
                    p,
                    P::Particle
                        | P::Copula
                        | P::Auxiliary
                        | P::AuxiliaryVerb
                        | P::AuxiliaryAdjective
                        | P::Expression
                        | P::Conjunction
                )
            })
        });
        let hiragana = surface.chars().all(japanese_utils::is_hiragana);
        if in_point || (hiragana && (w.len <= 2 || function_word)) || seen.contains(&w.entry.sequence) {
            continue;
        }
        seen.push(w.entry.sequence);
        let reading = w.entry.primary_reading().to_string();
        let word = if surface.chars().any(japanese_utils::is_kanji) {
            w.entry.headword().to_string()
        } else {
            reading.clone()
        };
        let gloss = w
            .entry
            .senses
            .first()
            .map(|s| {
                s.glosses
                    .iter()
                    .take(2)
                    .map(|g| g.text.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push(WordHint {
            reading: if reading == word { String::new() } else { reading },
            word,
            gloss,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_hint_leaves_out_the_point_and_particles() {
        use super::{Example, WordHint, hint_words};
        use crate::platform::SentenceWord;
        use grammar_core::types::parse_marked;
        use jmdict_types::{Gloss, KanjiElement, PartOfSpeech as P, ReadingElement, Sense, WordEntry};

        let entry = |seq, kanji: &str, reading: &str, pos: P, gloss: &str| WordEntry {
            sequence: seq,
            kanji_forms: if kanji.is_empty() { vec![] } else { vec![KanjiElement::from_text(kanji)] },
            reading_forms: vec![ReadingElement::from_reading(reading)],
            senses: vec![Sense {
                pos: vec![pos],
                glosses: vec![Gloss { text: gloss.into(), gloss_type: None }],
                ..Sense::default()
            }],
        };
        let word = |start, len, entry| SentenceWord { start, len, entry };
        // 傘を持ってき{てよかった}。
        let example = Example {
            ja: parse_marked("傘を持ってき{てよかった}。").unwrap(),
            ..Example::default()
        };
        let words = [
            word(0, 1, entry(1, "傘", "かさ", P::Noun, "umbrella")),
            word(1, 1, entry(2, "", "を", P::Particle, "object marker")),
            word(2, 5, entry(3, "持って来る", "もってくる", P::VerbKuru, "to bring")),
            word(7, 4, entry(4, "余割", "よかつ", P::Noun, "cosecant")),
            word(0, 1, entry(1, "傘", "かさ", P::Noun, "umbrella")),
        ];
        let hint = |w: &str, r: &str, g: &str| WordHint {
            word: w.into(),
            reading: r.into(),
            gloss: g.into(),
        };
        assert_eq!(
            hint_words(&words, &example),
            [
                hint("傘", "かさ", "umbrella"),
                hint("持って来る", "もってくる", "to bring"),
            ]
        );
    }
}
