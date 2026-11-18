//! How a production answer is compared with its reference on the card's
//! back: which chars differ ([`sentence_diff`]), the topic that may go
//! unsaid ([`leading_topic`]), the closest `also` sentence
//! ([`closest_also`]) and how polite each is ([`register`]). A help for the
//! eye, never a grade: the check is `grammar_core`'s, the rating the user's.

use std::ops::Range;

use super::reading::reading_keys;
use super::{Example, Span, plain};

/// What a sentence is compared by: `(char index, char)` pairs, several
/// per char when a kanji stands for its reading.
pub(super) type Keys = Vec<(usize, char)>;

/// The sentence's own chars, spaces and punctuation left out.
fn char_keys(text: &str) -> Keys {
    text.chars().enumerate().filter(|&(_, c)| !ignored(c)).collect()
}

/// Which chars of `a_text` and `b_text` the other lacks, compared by
/// their keys: one flag per char of each, in order, a char marked when one
/// of its keys has no match in the other sentence. The sentences are lined
/// up on their longest common run of keys.
fn keyed_diff(a_text: &str, ka: &Keys, b_text: &str, kb: &Keys) -> (Vec<bool>, Vec<bool>) {
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
    let mut in_a = vec![false; a_text.chars().count()];
    let mut in_b = vec![false; b_text.chars().count()];
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

/// `lcs[i][j]`, 0 past the edges.
fn cell(lcs: &[Vec<u16>], i: usize, j: usize) -> u16 {
    lcs.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0)
}

/// What the card's back shows of two sentences: [`sentence_diff`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// One flag per char of the first sentence: the second lacks it.
    pub a: Vec<bool>,
    /// The same for the second sentence.
    pub b: Vec<bool>,
    /// The chars of each sentence left out of the comparison: the optional
    /// topic (`leading_topic`), when the other sentence doesn't have the
    /// same one.
    pub a_optional: Option<Range<usize>>,
    pub b_optional: Option<Range<usize>>,
}

impl Diff {
    /// How many chars are marked, both sentences together.
    pub fn marks(&self) -> usize {
        self.a.iter().chain(&self.b).filter(|f| **f).count()
    }
}

/// The diff of a production answer `a` and a sentence `b` it is compared
/// with, for marking on the card's back. Spaces and punctuation are never
/// marked, so a missing 。 doesn't count; another valid word order is
/// marked too. By reading when both sentences' words are given, else by
/// chars. By reading, a kanji the other sentence lacks stands
/// for its reading (`reading_keys`), so 持って来て / 持ってきて or 子供 /
/// 子ども aren't marked; only those kanji, so a word the dictionary reads
/// wrong (今日は as こんにちは) costs nothing when both write it alike. With
/// `optional_topic` ([`topic_is_optional`]), a 私は-like topic one of them
/// starts with and the other lacks is left out, neither marked nor lined
/// up: Japanese drops it, and keeping it is right too.
pub fn sentence_diff(
    a: &str,
    wa: Option<&[crate::platform::SentenceWord]>,
    b: &str,
    wb: Option<&[crate::platform::SentenceWord]>,
    optional_topic: bool,
) -> Diff {
    let (ka, kb) = match (wa, wb) {
        (Some(wa), Some(wb)) => (
            reading_keys(a, wa, |c| b.contains(c)),
            reading_keys(b, wb, |c| a.contains(c)),
        ),
        _ => (char_keys(a), char_keys(b)),
    };
    let (ta, tb) = if optional_topic {
        match (leading_topic(a), leading_topic(b)) {
            (Some(x), Some(y)) if a.chars().skip(x.start).take(x.len()).eq(b.chars().skip(y.start).take(y.len())) => (None, None),
            other => other,
        }
    } else {
        (None, None)
    };
    let outside = |keys: Keys, skip: &Option<Range<usize>>| -> Keys {
        keys.into_iter().filter(|(i, _)| !skip.as_ref().is_some_and(|r| r.contains(i))).collect()
    };
    let (ma, mb) = keyed_diff(a, &outside(ka, &ta), b, &outside(kb, &tb));
    Diff { a: ma, b: mb, a_optional: ta, b_optional: tb }
}

/// Who a sentence's topic can be when it goes without saying: the speaker
/// or the listener. Longest first. が only for the speaker: あなたが is as
/// often the object (あなたが好きです, "I like you").
const TOPICS: &[(&str, &[char])] = &[
    ("私たち", &['は', 'が']),
    ("私達", &['は', 'が']),
    ("わたしたち", &['は', 'が']),
    ("僕たち", &['は', 'が']),
    ("僕達", &['は', 'が']),
    ("ぼくたち", &['は', 'が']),
    ("僕ら", &['は', 'が']),
    ("ぼくら", &['は', 'が']),
    ("俺たち", &['は', 'が']),
    ("俺達", &['は', 'が']),
    ("俺ら", &['は', 'が']),
    ("あなたたち", &['は']),
    ("あなた達", &['は']),
    ("あなたがた", &['は']),
    ("わたし", &['は', 'が']),
    ("あなた", &['は']),
    ("私", &['は', 'が']),
    ("僕", &['は', 'が']),
    ("ぼく", &['は', 'が']),
    ("俺", &['は', 'が']),
    ("おれ", &['は', 'が']),
    ("君", &['は']),
    ("きみ", &['は']),
];

/// The chars of a 私は / 僕が / あなたは the sentence starts with (spaces
/// aside), with a 、 after it.
fn leading_topic(text: &str) -> Option<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.iter().take_while(|c| c.is_whitespace()).count();
    let rest = chars.get(start..)?;
    TOPICS.iter().find_map(|(who, particles)| {
        let who: Vec<char> = who.chars().collect();
        let after = rest.strip_prefix(who.as_slice())?;
        let (&particle, after) = after.split_first()?;
        if !particles.contains(&particle) {
            return None;
        }
        let comma = usize::from(after.first() == Some(&'、'));
        Some(start..start + who.len() + 1 + comma)
    })
}

/// Whether the example's topic can be left out of the comparison: not when
/// the point is in it (私{は}学生です, 私{たち}は…).
pub fn topic_is_optional(example: &Example) -> bool {
    let sentence = plain(&example.ja);
    let Some(topic) = leading_topic(&sentence) else {
        return true;
    };
    let mut at = 0;
    for span in &example.ja {
        let n = span.text().chars().count();
        if matches!(span, Span::Highlight { .. }) && at < topic.end && topic.start < at + n {
            return false;
        }
        at += n;
    }
    true
}

/// The `also` sentence of `example` closest to a production `answer`: the
/// one `diff` ([`sentence_diff`]) marks the fewest chars against, when that
/// is fewer than the example's own sentence. `None` keeps the example's
/// sentence, also on a tie.
pub fn closest_also<'a>(
    answer: &str,
    example: &'a Example,
    diff: impl Fn(&str, &str) -> Diff,
) -> Option<&'a str> {
    let marks = |sentence: &str| diff(answer, sentence).marks();
    let own = marks(&plain(&example.ja));
    example
        .also
        .iter()
        .map(|s| (marks(s), s.as_str()))
        .filter(|&(m, _)| m < own)
        .min_by_key(|&(m, _)| m)
        .map(|(_, s)| s)
}

/// Spaces and punctuation, which the diff doesn't compare.
pub(super) fn ignored(c: char) -> bool {
    c.is_whitespace() || c.is_ascii_punctuation() || "。、！？「」『』（）・…〜～".contains(c)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The diff by chars, the topic compared like the rest.
    fn answer_diff(a: &str, b: &str) -> (Vec<bool>, Vec<bool>) {
        let d = sentence_diff(a, None, b, None, false);
        (d.a, d.b)
    }

    /// The marked chars of `text`, by `flags`.
    fn marked(text: &str, flags: &[bool]) -> String {
        text.chars()
            .zip(flags)
            .filter(|(_, f)| **f)
            .map(|(c, _)| c)
            .collect()
    }

    #[test]
    fn a_leading_i_or_you_is_optional() {
        use super::{leading_topic, sentence_diff, topic_is_optional};
        use grammar_core::types::parse_marked;

        assert_eq!(leading_topic("私はイタリアに住みたい。"), Some(0..2));
        assert_eq!(leading_topic("僕たちは、行く"), Some(0..5));
        assert_eq!(leading_topic(" あなたは学生ですか"), Some(1..5));
        // あなたが is as often the object; 私の is no topic; 彼は is content.
        assert_eq!(leading_topic("あなたが好きです"), None);
        assert_eq!(leading_topic("私の母"), None);
        assert_eq!(leading_topic("彼は来ない"), None);

        let d = sentence_diff("イタリアに住みたい", None, "私はイタリアに住みたい。", None, true);
        assert_eq!(marked("私はイタリアに住みたい。", &d.b), "");
        assert_eq!((d.a_optional, d.b_optional), (None, Some(0..2)));
        // Another I is left out on both sides; the same one is compared.
        let d = sentence_diff("僕はイタリアに住みたい", None, "私はイタリアに住みたい。", None, true);
        assert_eq!(d.marks(), 0);
        assert_eq!((d.a_optional, d.b_optional), (Some(0..2), Some(0..2)));
        let d = sentence_diff("私はイタリアに住みたい", None, "私はイタリアに住みたい。", None, true);
        assert_eq!((d.marks(), d.a_optional, d.b_optional), (0, None, None));
        // Not optional: marked as before.
        let d = sentence_diff("イタリアに住みたい", None, "私はイタリアに住みたい。", None, false);
        assert_eq!(marked("私はイタリアに住みたい。", &d.b), "私は");

        let example = |ja| Example { ja: parse_marked(ja).unwrap(), ..Example::default() };
        assert!(topic_is_optional(&example("私はイタリアに住み{たい}。")));
        assert!(topic_is_optional(&example("イタリアに住み{たい}。")));
        assert!(!topic_is_optional(&example("私{は}学生です。")));
        assert!(!topic_is_optional(&example("私{たち}は明日出発します。")));
    }

    #[test]
    fn the_closest_also_sentence_is_picked() {
        let example = Example {
            ja: grammar_core::types::parse_marked("時間に間に合っ{てよかった}。").unwrap(),
            also: vec!["間に合ってよかった。".into(), "間に合ってよかったです。".into()],
            ..Example::default()
        };
        let closest = |answer, example: &Example| closest_also(answer, example, |a, b| super::sentence_diff(a, None, b, None, false)).map(str::to_owned);
        assert_eq!(closest("間に合ってよかった", &example).as_deref(), Some("間に合ってよかった。"));
        assert_eq!(closest("間に合ってよかったです", &example).as_deref(), Some("間に合ってよかったです。"));
        assert_eq!(closest("時間に間に合ってよかった", &example), None);
        // A tie keeps the example's own sentence.
        assert_eq!(closest("よかった", &Example { also: vec!["時間に間に合ってよかった！".into()], ..example }), None);
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
    fn kanji_and_kana_spellings_of_a_word_match() {
        use super::sentence_diff;
        use crate::platform::SentenceWord;
        use jmdict_types::{KanjiElement, ReadingElement, WordEntry};

        let entry = |kanji: &str, reading: &str| WordEntry {
            kanji_forms: vec![KanjiElement::from_text(kanji)],
            reading_forms: vec![ReadingElement::from_reading(reading)],
            sequence: 0,
            senses: vec![],
        };
        let word = |start, len, kanji, reading| SentenceWord { start, len, entry: entry(kanji, reading) };
        // Each sentence with the words the dictionary finds in it.
        let diff = |a: &str, wa: &[SentenceWord], b: &str, wb: &[SentenceWord]| {
            let d = sentence_diff(a, Some(wa), b, Some(wb), false);
            let (x, y) = (d.a, d.b);
            (marked(a, &x), marked(b, &y))
        };
        let none = String::new;

        // 持って来て, 持ってきて: the dictionary form is 持って来る.
        // As JMdict has it: 持ってくる first, which doesn't write 来.
        let entry2 = |forms: &[&str], reading: &str| WordEntry {
            kanji_forms: forms.iter().map(|&f| KanjiElement::from_text(f)).collect(),
            ..entry("", reading)
        };
        let both = || SentenceWord { start: 2, len: 5, entry: entry2(&["持ってくる", "持って来る"], "もってくる") };
        let (kanji, kana) = ([both()], [both()]);
        assert_eq!(diff("傘を持って来てよかった", &kanji, "傘を持ってきてよかった。", &kana), (none(), none()));
        // 来ない reads こない.
        let w = [word(0, 3, "来る", "くる")];
        assert_eq!(diff("来ない", &w, "こない", &[]), (none(), none()));
        assert_eq!(diff("来ない", &w, "きない", &[]), ("来".into(), "き".into()));
        // 時 and とき; 間に合う, whose kana runs line up the reading.
        let a = [word(0, 1, "時", "とき")];
        assert_eq!(diff("時に", &a, "ときに", &[]), (none(), none()));
        let a = [word(0, 5, "間に合う", "まにあう")];
        assert_eq!(diff("間に合った", &a, "まにあった", &[]), (none(), none()));
        // A different word stays marked, and a kanji no word explains is
        // compared as written.
        assert_eq!(diff("時に", &a[..0], "ときに", &[]), ("時".into(), "とき".into()));
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
