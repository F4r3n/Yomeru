//! The grammar dictionary: Yomeru's own `data/grammar.toml`, compiled into
//! `grammar.bin` and shipped like JMdict. Lookups go through
//! [`crate::platform::DictClient::grammar_search`]: the server, the
//! Android app's bundled copy, or the extension's background.

pub use grammar_core::types::plain;
pub use grammar_core::{AnswerCheck, Example, GrammarEntry, Related, Rich, Span, Verdict};

/// Which chars of a production `answer` and its `reference` the other
/// lacks, for marking them on the card's back: one flag per char of each,
/// in order. The sentences are lined up on their longest common run of
/// chars; spaces and punctuation are never marked, so a missing 。 doesn't
/// count. A help for the eye, not a grade: another valid word order is
/// marked too.
pub fn answer_diff(answer: &str, reference: &str) -> (Vec<bool>, Vec<bool>) {
    let a: Vec<char> = answer.chars().collect();
    let b: Vec<char> = reference.chars().collect();
    // Compared chars only, with their position in the sentence.
    let ka: Vec<(usize, char)> = a.iter().copied().enumerate().filter(|&(_, c)| !ignored(c)).collect();
    let kb: Vec<(usize, char)> = b.iter().copied().enumerate().filter(|&(_, c)| !ignored(c)).collect();
    // lcs[i][j]: the common run of ka[i..] and kb[j..].
    let mut lcs = vec![vec![0u16; kb.len() + 1]; ka.len() + 1];
    for i in (0..ka.len()).rev() {
        for j in (0..kb.len()).rev() {
            let here = match (ka.get(i), kb.get(j)) {
                (Some(x), Some(y)) if x.1 == y.1 => cell(&lcs, i + 1, j + 1) + 1,
                _ => cell(&lcs, i + 1, j).max(cell(&lcs, i, j + 1)),
            };
            if let Some(c) = lcs.get_mut(i).and_then(|row| row.get_mut(j)) {
                *c = here;
            }
        }
    }
    let mut in_a = vec![false; a.len()];
    let mut in_b = vec![false; b.len()];
    let mark = |flags: &mut Vec<bool>, at: usize| {
        if let Some(f) = flags.get_mut(at) {
            *f = true;
        }
    };
    let (mut i, mut j) = (0, 0);
    while let (Some(x), Some(y)) = (ka.get(i), kb.get(j)) {
        if x.1 == y.1 {
            i += 1;
            j += 1;
        } else if cell(&lcs, i + 1, j) >= cell(&lcs, i, j + 1) {
            mark(&mut in_a, x.0);
            i += 1;
        } else {
            mark(&mut in_b, y.0);
            j += 1;
        }
    }
    for x in ka.get(i..).unwrap_or_default() {
        mark(&mut in_a, x.0);
    }
    for y in kb.get(j..).unwrap_or_default() {
        mark(&mut in_b, y.0);
    }
    (in_a, in_b)
}

fn cell(lcs: &[Vec<u16>], i: usize, j: usize) -> u16 {
    lcs.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0)
}

/// Spaces and punctuation, which [`answer_diff`] doesn't compare.
fn ignored(c: char) -> bool {
    c.is_whitespace() || c.is_ascii_punctuation() || "。、！？「」『』（）・…〜～".contains(c)
}

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

/// How polite a sentence is, by its verbs and copula: です/ます or plain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register {
    Polite,
    Plain,
}

/// The [`Register`] of `sentence`: polite when a です or ます form is in it
/// (ます, ました, ません, ましょう, です, でした, でしょう) or ください.
pub fn register(sentence: &str) -> Register {
    // ますます, "more and more", isn't a ます.
    let s = sentence.replace("ますます", "");
    let polite = ["ます", "ました", "ません", "ましょう", "です", "でした", "でしょう", "ください"]
        .iter()
        .any(|p| s.contains(p));
    if polite { Register::Polite } else { Register::Plain }
}

/// Deletes the grammar dictionary earlier versions downloaded and converted
/// on the device (bee-san's, never ours to keep around). Once per device;
/// harmless when there is none.
pub async fn remove_old_download() {
    const DONE_KEY: &str = "yomeru_old_grammar_removed";
    if crate::rt::kv_get::<bool>(DONE_KEY).unwrap_or(false) {
        return;
    }
    match old::delete().await {
        Ok(()) => {
            if let Err(e) = crate::rt::kv_set(DONE_KEY, &true) {
                tracing::warn!("[yomeru] {e}");
            }
        }
        Err(e) => tracing::warn!("[yomeru] removing the old grammar dictionary: {e}"),
    }
}

/// Its own IndexedDB database, `yomeru-grammar`.
#[cfg(target_arch = "wasm32")]
mod old {
    pub(super) async fn delete() -> Result<(), String> {
        let err = |e: idb::Error| format!("{e:?}");
        idb::Factory::new()
            .map_err(err)?
            .delete("yomeru-grammar")
            .map_err(err)?
            .await
            .map_err(err)
    }
}

/// `grammar.bin` in the app's data dir.
#[cfg(not(target_arch = "wasm32"))]
mod old {
    pub(super) async fn delete() -> Result<(), String> {
        let path = crate::rt::data_dir().join("grammar.bin");
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("deleting {}: {e}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::answer_diff;

    /// The marked chars of `text`, by `flags`.
    fn marked(text: &str, flags: &[bool]) -> String {
        text.chars()
            .zip(flags)
            .filter(|(_, f)| **f)
            .map(|(c, _)| c)
            .collect()
    }

    #[test]
    fn the_extra_and_missing_chars_are_marked() {
        let (answer, reference) = ("傘を持ってきてよかっただった", "傘を持ってきてよかった。");
        let (a, b) = answer_diff(answer, reference);
        assert_eq!(marked(answer, &a), "だった");
        // The 。 isn't marked missing.
        assert_eq!(marked(reference, &b), "");

        let (answer, reference) = ("雨が降ったら行かない", "雨が降ったら、行きません。");
        let (a, b) = answer_diff(answer, reference);
        assert_eq!(marked(answer, &a), "かない");
        assert_eq!(marked(reference, &b), "きません");
    }

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

    #[test]
    fn polite_and_plain_sentences() {
        use super::{Register::*, register};
        for (s, r) in [
            ("学生です。", Polite),
            ("昨日は雨でした。", Polite),
            ("明日行きます。", Polite),
            ("行きませんでした。", Polite),
            ("一緒に帰りましょう。", Polite),
            ("学生だ。", Plain),
            ("傘を持ってきてよかった。", Plain),
            ("ますます寒くなる。", Plain),
            ("窓を閉めてください。", Polite),
            ("窓を閉めて。", Plain),
        ] {
            assert_eq!(register(s), r, "{s}");
        }
    }

    #[test]
    fn the_same_sentence_marks_nothing() {
        let (a, b) = answer_diff("駅まで歩きます", "駅まで 歩きます。");
        assert!(a.iter().chain(&b).all(|f| !f));
        let (a, b) = answer_diff("", "駅");
        assert!(a.is_empty());
        assert_eq!(b, [true]);
    }
}
