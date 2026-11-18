//! A sentence read by sound, for the diff: the kanji of its words as their
//! kana, from the words JMdict finds in it.

use super::Keys;
use super::compare::ignored;

/// The sentence compared by sound: each kanji of a word `words` found
/// ([`crate::platform::DictClient::sentence_words`] of this `text`) stands
/// for its reading, so 持って来て and 持ってきて, or 時 and とき, match;
/// but a kanji `keep` names stays as written. Other chars are kept as
/// they are, like [`char_keys`].
pub(super) fn reading_keys(
    text: &str,
    words: &[crate::platform::SentenceWord],
    keep: impl Fn(char) -> bool,
) -> Keys {
    let chars: Vec<char> = text.chars().collect();
    // The reading of each char a word's kanji covers, by char index.
    let mut read: Vec<Option<Keys>> = vec![None; chars.len()];
    for w in words {
        let Some(surface) = chars.get(w.start..w.start + w.len) else {
            continue;
        };
        let after = chars.get(w.start + w.len).copied();
        for (at, kana) in word_reading(surface, after, &w.entry) {
            let i = w.start + at;
            if chars.get(i).is_some_and(|&c| keep(c)) {
                continue;
            }
            if let Some(slot) = read.get_mut(i) {
                *slot = Some(kana.into_iter().map(|c| (i, c)).collect());
            }
        }
    }
    let mut out = Keys::new();
    for (i, &c) in chars.iter().enumerate() {
        match read.get_mut(i).and_then(Option::take) {
            Some(keys) => out.extend(keys),
            None if !ignored(c) => out.push((i, c)),
            None => {}
        }
    }
    out
}

/// The readings of a word's kanji in its `surface`: `(char of the surface,
/// its kana)` for each kanji char the entry's written form explains, the
/// form explaining the most (持って来る over 持ってくる for 持って来);
/// `after` is the sentence's char after the word. Empty when no written
/// form lines up with a reading.
fn word_reading(
    surface: &[char],
    after: Option<char>,
    entry: &jmdict_types::WordEntry,
) -> Vec<(usize, Vec<char>)> {
    use japanese_utils::is_kanji;
    let kanji = |c: char| is_kanji(c) || c == '々' || c == 'ヶ';
    if !surface.iter().any(|&c| kanji(c)) {
        return vec![];
    }
    let mut best: Vec<(usize, Vec<char>)> = vec![];
    for form in &entry.kanji_forms {
        let runs = split_runs(&form.text, kanji);
        // The form must write the surface's kanji: its first kanji run
        // is in the surface.
        let Some(first) = runs.iter().find(|(k, _)| *k).map(|(_, r)| r) else {
            continue;
        };
        if find(surface, first, 0).is_none() {
            continue;
        }
        for reading in &entry.reading_forms {
            let reading: Vec<char> = reading.text.chars().map(hiragana).collect();
            let Some(parts) = align(&runs, &reading) else {
                continue;
            };
            let mut out = Vec::new();
            let mut from = 0;
            for ((_, run), part) in runs.iter().filter(|(k, _)| *k).zip(parts) {
                let Some(at) = find(surface, run, from) else {
                    break;
                };
                let next = match surface.get(at + run.len()) {
                    Some(&c) => Some(c),
                    None => after,
                };
                let part = if run.as_slice() == ['来'] && part == ['く'] {
                    // 来る's reading moves with its ending: 来て, 来ない.
                    match next {
                        Some('る' | 'れ') | None => vec!['く'],
                        Some('て' | 'た' | 'ま' | 'そ') => vec!['き'],
                        Some(_) => vec!['こ'],
                    }
                } else {
                    part
                };
                // Spread the kana over the run's chars, in order.
                let n = run.len();
                for k in 0..n {
                    let lo = k * part.len() / n;
                    let hi = (k + 1) * part.len() / n;
                    out.push((at + k, part.get(lo..hi).unwrap_or_default().to_vec()));
                }
                from = at + n;
            }
            if out.len() > best.len() {
                best = out;
            }
            // The first reading that lines up is the form's.
            break;
        }
    }
    best
}

/// `text` cut into runs of kanji and of other chars: `(is kanji, chars)`.
fn split_runs(text: &str, kanji: impl Fn(char) -> bool) -> Vec<(bool, Vec<char>)> {
    let mut runs: Vec<(bool, Vec<char>)> = Vec::new();
    for c in text.chars() {
        let k = kanji(c);
        match runs.last_mut() {
            Some((last, run)) if *last == k => run.push(c),
            _ => runs.push((k, vec![c])),
        }
    }
    runs
}

/// The kana each kanji run of `runs` reads as in `reading`, the kana runs
/// matching it as they are; `None` when they don't line up.
fn align(runs: &[(bool, Vec<char>)], reading: &[char]) -> Option<Vec<Vec<char>>> {
    let Some(((is_kanji, run), rest)) = runs.split_first() else {
        return reading.is_empty().then(Vec::new);
    };
    if !is_kanji {
        let run: Vec<char> = run.iter().copied().map(hiragana).collect();
        return reading.strip_prefix(run.as_slice()).and_then(|r| align(rest, r));
    }
    // At least a kana per kanji; the shortest reading that lets the rest
    // line up.
    (run.len()..=reading.len()).find_map(|n| {
        let (head, tail) = reading.split_at_checked(n)?;
        let mut parts = align(rest, tail)?;
        parts.insert(0, head.to_vec());
        Some(parts)
    })
}

/// Where `run` is in `surface`, from `from` on.
fn find(surface: &[char], run: &[char], from: usize) -> Option<usize> {
    if run.is_empty() {
        return None;
    }
    (from..surface.len()).find(|&i| surface.get(i..i + run.len()) == Some(run))
}

/// Katakana as hiragana; other chars unchanged.
fn hiragana(c: char) -> char {
    match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}
