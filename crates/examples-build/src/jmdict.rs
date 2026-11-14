//! Uses JMdict to reach examples the corpus files under another form of the
//! word (為る vs する, 事 vs こと), or never tags at all.

use crate::{
    Candidate, Index, MAX_PER_HEADWORD, Parsed, char_len, commonness, sentence_penalty,
};
use aho_corasick::AhoCorasick;
use examples_types::ExampleRef;
use jmdict_types::{Freq, FreqKind, Misc, PartOfSpeech, WordEntry};
use std::collections::{BTreeMap, HashMap};

/// Forms with fewer examples than this are topped up from the word's best form.
const TOP_UP_BELOW: usize = 5;

/// Literal matches kept per word before ranking.
const LITERAL_CAP: usize = 2000;

/// Lower is more common, `u32::MAX`: no priority tag. Same scale as the
/// app's frequency label (≤ 2: "Top 1k", ≤ 24: "Common").
fn rank(e: &WordEntry) -> u32 {
    let tag = |t: &Freq| match t.kind {
        FreqKind::Nf => u32::from(t.value),
        _ if t.value == 1 => 1,
        _ => 24,
    };
    e.kanji_forms
        .iter()
        .flat_map(|k| &k.priorities)
        .chain(e.reading_forms.iter().flat_map(|r| &r.priorities))
        .map(tag)
        .min()
        .unwrap_or(u32::MAX)
}

fn forms(e: &WordEntry) -> impl Iterator<Item = &str> {
    e.kanji_forms
        .iter()
        .map(|k| k.text.as_str())
        .chain(e.reading_forms.iter().map(|r| r.text.as_str()))
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{30FF}')
}

fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{309F}')
}

fn conjugates(e: &WordEntry) -> bool {
    use PartOfSpeech as P;
    e.senses.iter().flat_map(|s| &s.pos).any(|p| {
        matches!(
            p,
            P::Adjective
                | P::VerbIchidan
                | P::VerbIchidanS
                | P::VerbGodanBu
                | P::VerbGodanGu
                | P::VerbGodanKu
                | P::VerbGodanKuS
                | P::VerbGodanMu
                | P::VerbGodanNu
                | P::VerbGodanRu
                | P::VerbGodanRuIrr
                | P::VerbGodanSu
                | P::VerbGodanTsu
                | P::VerbGodanU
                | P::VerbGodanUS
        )
    })
}

/// Strings whose presence in a sentence means it likely uses the word. Short
/// kana strings are skipped (とろ is inside ところ); conjugating words also
/// match on their stem (面白い → 面白かった).
fn needles(e: &WordEntry) -> Vec<String> {
    let stem_ok = conjugates(e);
    let mut out = Vec::new();
    for form in forms(e) {
        let n = form.chars().count();
        let kana_only = form.chars().all(is_kana);
        if n >= if kana_only { 3 } else { 2 } {
            out.push(form.to_string());
        }
        if stem_ok && !kana_only && n >= 3 && form.chars().last().is_some_and(is_hiragana) {
            out.push(form.chars().take(n - 1).collect());
        }
    }
    out
}

/// Common words with no tagged example under any form get the sentences that
/// contain them as written, keyed by their first form.
pub(crate) fn add_literal_candidates(
    entries: &[WordEntry],
    sentences: &[Parsed],
    doc_freq: &HashMap<&str, u32>,
    candidates: &mut BTreeMap<String, Vec<Candidate>>,
) -> anyhow::Result<()> {
    let missing: Vec<&WordEntry> = entries
        .iter()
        .filter(|e| rank(e) != u32::MAX && !forms(e).any(|f| candidates.contains_key(f)))
        .collect();

    let mut needle_ids: HashMap<String, usize> = HashMap::new();
    let mut owners: Vec<Vec<usize>> = Vec::new();
    for (i, e) in missing.iter().enumerate() {
        for n in needles(e) {
            let id = *needle_ids.entry(n).or_insert_with(|| {
                owners.push(Vec::new());
                owners.len() - 1
            });
            if let Some(o) = owners.get_mut(id)
                && o.last() != Some(&i)
            {
                o.push(i);
            }
        }
    }
    let mut patterns: Vec<(&String, usize)> = needle_ids.iter().map(|(n, &id)| (n, id)).collect();
    patterns.sort_by_key(|&(_, id)| id);
    let ac = AhoCorasick::new(patterns.iter().map(|(n, _)| n.as_str()))?;

    let mut found: Vec<Vec<Candidate>> = vec![Vec::new(); missing.len()];
    for (si, s) in sentences.iter().enumerate() {
        let mut base = None;
        for m in ac.find_overlapping_iter(&s.japanese) {
            let Some(start) = s.japanese.get(..m.start()) else {
                continue;
            };
            let Some(text) = s.japanese.get(m.range()) else {
                continue;
            };
            let base = *base.get_or_insert_with(|| {
                commonness(&s.tokens, "", doc_freq) - sentence_penalty(&s.japanese)
            });
            for &o in owners.get(m.pattern().as_usize()).into_iter().flatten() {
                let Some(list) = found.get_mut(o) else {
                    continue;
                };
                if list.len() >= LITERAL_CAP || list.last().is_some_and(|c| c.sentence == si) {
                    continue;
                }
                list.push(Candidate {
                    sentence: si,
                    base,
                    pattern: (None, None),
                    mark: (char_len(start), char_len(text)),
                });
            }
        }
    }

    for (e, list) in missing.into_iter().zip(found) {
        if let Some(key) = forms(e).next()
            && !list.is_empty()
        {
            candidates.insert(key.to_string(), list);
        }
    }
    Ok(())
}

/// The examples of all forms of a word, best-covered form first.
fn merged_examples(e: &WordEntry, index: &Index) -> Option<Vec<ExampleRef>> {
    let mut lists: Vec<&Vec<ExampleRef>> = forms(e).filter_map(|f| index.get(f)).collect();
    lists.sort_by_key(|l| std::cmp::Reverse(l.len()));
    let mut out: Vec<ExampleRef> = Vec::new();
    for r in lists.into_iter().flatten() {
        if out.len() >= MAX_PER_HEADWORD {
            break;
        }
        if !out.iter().any(|o| o.offset == r.offset) {
            out.push(*r);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Every form of a word without (enough) examples of its own gets those of
/// all its forms. Kanji forms and kana headwords claim a key first, then
/// readings of kanji words, the more common word first (箸 and 橋 share はし).
pub(crate) fn add_aliases(entries: &[WordEntry], index: &mut Index) {
    let sources: Vec<Option<Vec<ExampleRef>>> =
        entries.iter().map(|e| merged_examples(e, index)).collect();

    let mut claims: Vec<(bool, u32, usize, &str)> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let r = rank(e);
        let usually_kana = e
            .senses
            .first()
            .is_some_and(|s| s.misc.contains(&Misc::UsuallyKana));
        let kana_headword = e.kanji_forms.is_empty() || usually_kana;
        for k in &e.kanji_forms {
            claims.push((false, r, i, &k.text));
        }
        // Readings of rare kanji words are too ambiguous to be worth a key.
        if kana_headword || r != u32::MAX {
            for rd in &e.reading_forms {
                claims.push((!kana_headword, r, i, &rd.text));
            }
        }
    }
    claims.sort_by_key(|&(second, r, _, _)| (second, r));

    for (second, _, i, form) in claims {
        let Some(Some(refs)) = sources.get(i) else {
            continue;
        };
        match index.get_mut(form) {
            None => {
                index.insert(form.to_string(), refs.clone());
            }
            // A form tagged only a few times (ない next to 無い) borrows the rest.
            Some(own) if !second && own.len() < TOP_UP_BELOW => {
                for r in refs {
                    if own.len() >= MAX_PER_HEADWORD {
                        break;
                    }
                    if !own.iter().any(|o| o.offset == r.offset) {
                        own.push(*r);
                    }
                }
            }
            Some(_) => {}
        }
    }
}

pub(crate) struct Coverage {
    pub top: usize,
    pub top_with: usize,
    pub common: usize,
    pub common_with: usize,
}

/// How many "Top 1k" and common words have at least one example.
pub(crate) fn coverage(entries: &[WordEntry], index: &Index) -> Coverage {
    let mut c = Coverage {
        top: 0,
        top_with: 0,
        common: 0,
        common_with: 0,
    };
    for e in entries {
        let r = rank(e);
        if r > 24 {
            continue;
        }
        let has = forms(e).any(|f| index.contains_key(f));
        c.common += 1;
        c.common_with += usize::from(has);
        if r <= 2 {
            c.top += 1;
            c.top_with += usize::from(has);
        }
    }
    c
}
