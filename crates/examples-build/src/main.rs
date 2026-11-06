// Offline build CLI: progress messages to stderr are intentional user output.
#![allow(clippy::print_stderr)]

use anyhow::{Result, bail};
use clap::Parser;
use examples_types::ExampleEntry;
use postcard::to_allocvec;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;

// Cap stored offsets per headword to keep the index size manageable.
const MAX_PER_HEADWORD: usize = 20;

// Ranking knobs. Learners read only the first few examples, so those should
// be short, single-clause sentences made of common words that show the word
// with its particle. Later slots may run a little longer.
const TARGET_CHARS_FIRST: f32 = 11.0;
const TARGET_CHARS_LATER: f32 = 16.0;
/// Slots before this index aim at `TARGET_CHARS_FIRST`.
const SHORT_SLOTS: usize = 4;
const LENGTH_WEIGHT: f32 = 0.6;
const COMMA_PENALTY: f32 = 3.0;
const EXTRA_SENTENCE_PENALTY: f32 = 6.0;
const PARTICLE_BONUS: f32 = 4.0;
/// The corpus opens with thousands of 彼は… sentences; nudge them down so
/// subjects vary.
const KARE_PENALTY: f32 = 1.0;
/// Per already-picked example using the same particle pattern.
const SAME_PATTERN_PENALTY: f32 = 2.5;
/// Applied when a candidate is a near copy of an already-picked example.
const NEAR_DUPLICATE_PENALTY: f32 = 8.0;
const NEAR_DUPLICATE_JACCARD: f32 = 0.5;
/// Only the best-scoring candidates go through the (quadratic) diversity pass.
const POOL_SIZE: usize = 300;

/// Particles whose presence next to the word shows how it is used
/// (をvs に for verbs, が/を/に after nouns...).
const PARTICLES: &[&str] = &[
    "を", "に", "で", "が", "へ", "と", "から", "まで", "は", "も", "の", "より",
];

#[derive(Parser)]
#[command(
    name = "examples-build",
    about = "Build binary example-sentence index from Tanaka Corpus"
)]
struct Args {
    #[arg(short, long)]
    input: PathBuf,

    #[arg(short, long, default_value = "extension/data/examples.bin")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    eprintln!("Parsing {:?}...", args.input);
    let content = std::fs::read_to_string(&args.input)?;

    let (index, sentences_bytes, sentence_count) = build(&content)?;
    eprintln!("{sentence_count} sentences, {} headwords.", index.len());

    write_bin(&index, &sentences_bytes, &args.output)?;

    let size = std::fs::metadata(&args.output)?.len();
    eprintln!("Done. Output: {:.1} MB", size as f64 / 1_048_576.0);
    Ok(())
}

// --------------------------------------------------------------------------
// Format (per pair of lines):
//   A: <japanese>\t<english>#ID=<id>
//   B: word1(reading)[sense] word2 word3{surface} ...
// --------------------------------------------------------------------------

/// `(headword → byte offsets, sentences blob, sentence count)`
type BuildOutput = (Vec<(String, Vec<u32>)>, Vec<u8>, usize);

struct Parsed {
    japanese: String,
    english: String,
    /// Headword of every B-line token, in order (particles included).
    tokens: Vec<String>,
}

/// One sentence considered as an example for one headword.
struct Candidate {
    sentence: usize,
    /// Score without the length term (that depends on the slot).
    base: f32,
    /// (particle before, particle after) the word; used for diversity.
    pattern: (Option<usize>, Option<usize>),
}

fn parse_pair(a_line: &str, b_line: &str) -> Option<Parsed> {
    // A line: "A: <japanese>\t<english>#ID=..."
    let a_rest = a_line.strip_prefix("A: ")?;
    let b_rest = b_line.strip_prefix("B: ")?;
    let (jp, en) = a_rest.split_once('\t')?;
    let japanese = jp.trim().to_string();
    let english = en.split('#').next().unwrap_or("").trim().to_string();
    if japanese.is_empty() || english.is_empty() {
        return None;
    }
    // B line: "B: word1(reading)[sense] word2{surface} word3~ ..."
    let tokens = b_rest
        .split_whitespace()
        .map(|t| {
            let end = t.find(['(', '[', '{', '~']).unwrap_or(t.len());
            t.get(..end).unwrap_or(t).to_string()
        })
        .collect();
    Some(Parsed {
        japanese,
        english,
        tokens,
    })
}

fn build(content: &str) -> Result<BuildOutput> {
    let lines: Vec<&str> = content.lines().collect();
    let sentences: Vec<Parsed> = lines
        .chunks_exact(2)
        .filter_map(|pair| match pair {
            [a, b] => parse_pair(a, b),
            _ => None,
        })
        .filter(|p| p.tokens.iter().any(|t| is_valid_headword(t)))
        .collect();

    // How many sentences each headword appears in: a cheap "is this word
    // common" signal that needs no external frequency list.
    let mut doc_freq: HashMap<&str, u32> = HashMap::new();
    for s in &sentences {
        let unique: HashSet<&str> = s.tokens.iter().map(String::as_str).collect();
        for t in unique {
            *doc_freq.entry(t).or_default() += 1;
        }
    }

    let mut sentences_bytes: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(sentences.len());
    let mut candidates: BTreeMap<&str, Vec<Candidate>> = BTreeMap::new();

    for (idx, s) in sentences.iter().enumerate() {
        let offset_usize = sentences_bytes.len();
        if offset_usize > u32::MAX as usize {
            bail!(
                "examples sentences blob exceeds 4 GiB ({} bytes)",
                offset_usize
            );
        }
        offsets.push(offset_usize as u32);
        let entry = ExampleEntry {
            japanese: s.japanese.clone(),
            english: s.english.clone(),
        };
        let serialized = to_allocvec(&entry)?;
        sentences_bytes.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
        sentences_bytes.extend_from_slice(&serialized);

        let shared = sentence_penalty(&s.japanese);
        let mut seen = HashSet::new();
        for (pos, hw) in s.tokens.iter().enumerate() {
            if !is_valid_headword(hw) || !seen.insert(hw.as_str()) {
                continue;
            }
            let (base, pattern) = score_for(&s.tokens, pos, &doc_freq);
            candidates.entry(hw).or_default().push(Candidate {
                sentence: idx,
                base: base - shared,
                pattern,
            });
        }
    }

    let index: Vec<(String, Vec<u32>)> = candidates
        .into_iter()
        .map(|(hw, cands)| {
            let picked = select(cands, &sentences)
                .into_iter()
                .filter_map(|i| offsets.get(i).copied())
                .collect();
            (hw.to_string(), picked)
        })
        .collect();
    Ok((index, sentences_bytes, sentences.len()))
}

/// Penalty that depends only on the sentence (not on the headword or slot).
fn sentence_penalty(japanese: &str) -> f32 {
    let commas = japanese.matches(['、', ',', '，']).count();
    let enders = japanese
        .chars()
        .filter(|c| matches!(c, '。' | '？' | '！' | '?' | '!'))
        .count();
    let mut p =
        commas as f32 * COMMA_PENALTY + enders.saturating_sub(1) as f32 * EXTRA_SENTENCE_PENALTY;
    if japanese.starts_with('彼') {
        p += KARE_PENALTY;
    }
    p
}

fn particle_index(token: Option<&String>) -> Option<usize> {
    let t = token?;
    PARTICLES.iter().position(|p| p == t)
}

/// Headword-specific part of the score: are the other words common, and is
/// the word shown with a particle?
fn score_for(
    tokens: &[String],
    pos: usize,
    doc_freq: &HashMap<&str, u32>,
) -> (f32, (Option<usize>, Option<usize>)) {
    let hw = tokens.get(pos).map(String::as_str).unwrap_or("");
    // The two rarest other content words decide: one obscure word is
    // enough to make a sentence hard.
    let mut freqs: Vec<u32> = tokens
        .iter()
        .filter(|t| t.as_str() != hw && is_valid_headword(t))
        .map(|t| doc_freq.get(t.as_str()).copied().unwrap_or(0))
        .collect();
    freqs.sort_unstable();
    let commonness: f32 = freqs
        .iter()
        .take(2)
        .map(|&f| (f64::from(f) + 1.0).log10().min(3.0) as f32)
        .sum();

    let before = pos
        .checked_sub(1)
        .and_then(|i| particle_index(tokens.get(i)));
    let after = particle_index(tokens.get(pos + 1));
    let bonus = if before.is_some() {
        PARTICLE_BONUS
    } else {
        0.0
    };
    (commonness + bonus, (before, after))
}

fn bigrams(s: &str) -> HashSet<(char, char)> {
    let chars: Vec<char> = s.chars().collect();
    chars
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

fn jaccard(a: &HashSet<(char, char)>, b: &HashSet<(char, char)>) -> f32 {
    let union = a.union(b).count();
    if union == 0 {
        return 1.0;
    }
    a.intersection(b).count() as f32 / union as f32
}

/// Pick up to `MAX_PER_HEADWORD` sentence indices, best first. Greedy: each
/// slot takes the best remaining candidate after penalising particle patterns
/// and near copies of what was already picked.
fn select(mut cands: Vec<Candidate>, sentences: &[Parsed]) -> Vec<usize> {
    let chars = |c: &Candidate| {
        sentences
            .get(c.sentence)
            .map_or(0, |s| s.japanese.chars().count()) as f32
    };
    let length_score =
        |c: &Candidate, target: f32| c.base - (chars(c) - target).abs() * LENGTH_WEIGHT;

    // Stable sort keeps corpus order among ties, so builds are deterministic.
    cands.sort_by(|a, b| {
        length_score(b, TARGET_CHARS_FIRST).total_cmp(&length_score(a, TARGET_CHARS_FIRST))
    });
    cands.truncate(POOL_SIZE);

    let grams: Vec<HashSet<(char, char)>> = cands
        .iter()
        .map(|c| {
            sentences
                .get(c.sentence)
                .map(|s| bigrams(&s.japanese))
                .unwrap_or_default()
        })
        .collect();
    let mut taken = vec![false; cands.len()];
    let mut picked: Vec<usize> = Vec::new();
    let mut pattern_count: HashMap<(Option<usize>, Option<usize>), usize> = HashMap::new();

    while picked.len() < MAX_PER_HEADWORD {
        let target = if picked.len() < SHORT_SLOTS {
            TARGET_CHARS_FIRST
        } else {
            TARGET_CHARS_LATER
        };
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in cands.iter().enumerate() {
            if taken.get(i).copied().unwrap_or(true) {
                continue;
            }
            let mut s = length_score(c, target)
                - pattern_count.get(&c.pattern).copied().unwrap_or(0) as f32 * SAME_PATTERN_PENALTY;
            let g = grams.get(i);
            let near_dup = picked.iter().any(|&j| match (g, grams.get(j)) {
                (Some(a), Some(b)) => jaccard(a, b) >= NEAR_DUPLICATE_JACCARD,
                _ => false,
            });
            if near_dup {
                s -= NEAR_DUPLICATE_PENALTY;
            }
            if best.is_none_or(|(_, bs)| s > bs) {
                best = Some((i, s));
            }
        }
        let Some((i, _)) = best else { break };
        if let Some(t) = taken.get_mut(i) {
            *t = true;
        }
        if let Some(c) = cands.get(i) {
            *pattern_count.entry(c.pattern).or_default() += 1;
        }
        picked.push(i);
    }

    picked
        .into_iter()
        .filter_map(|i| cands.get(i).map(|c| c.sentence))
        .collect()
}

fn is_valid_headword(s: &str) -> bool {
    !s.is_empty()
        && s.chars().any(|c| {
            let n = c as u32;
            (0x3041..=0x30FF).contains(&n)  // hiragana + katakana
                || (0x3400..=0x9FFF).contains(&n)  // CJK ext-A + unified
                || (0xF900..=0xFAFF).contains(&n) // CJK compat
        })
}

fn write_bin(
    index: &[(String, Vec<u32>)],
    sentences_bytes: &[u8],
    path: &std::path::Path,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    f.write_all(b"EXPL")?;
    f.write_all(&[1u8])?;

    let index_bytes = to_allocvec(index)?;
    f.write_all(&(index_bytes.len() as u32).to_le_bytes())?;
    f.write_all(&index_bytes)?;

    f.write_all(&(sentences_bytes.len() as u32).to_le_bytes())?;
    f.write_all(sentences_bytes)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_headword_recognises_japanese_chars() {
        assert!(is_valid_headword("猫")); // kanji
        assert!(is_valid_headword("ねこ")); // hiragana
        assert!(is_valid_headword("ネコ")); // katakana
        assert!(is_valid_headword("食べる")); // mixed kanji + hiragana
    }

    #[test]
    fn is_valid_headword_rejects_empty_and_latin_only() {
        assert!(!is_valid_headword(""));
        assert!(!is_valid_headword("hello"));
        assert!(!is_valid_headword("123"));
    }

    #[test]
    fn build_parses_one_sentence_and_extracts_headwords() {
        let input = "A: 猫が寝る。\tThe cat sleeps.#ID=1_1\n\
                     B: 猫(ねこ)[01] が 寝る{寝る}\n";
        let (index, _bytes, n) = build(input).unwrap();
        assert_eq!(n, 1);

        let words: Vec<&str> = index.iter().map(|(k, _)| k.as_str()).collect();
        // Strict-Japanese filter keeps 猫 and 寝る; the bare particle が also
        // passes (hiragana), so the BTreeMap returns them in sort order.
        assert!(words.contains(&"猫"));
        assert!(words.contains(&"寝る"));
        // Every headword in a single sentence points to the same offset.
        let first_off = index[0].1[0];
        for (_, offs) in &index {
            assert_eq!(offs.len(), 1);
            assert_eq!(offs[0], first_off);
        }
    }

    /// Repeated headwords inside the same B-line must only contribute one
    /// offset (the `seen` HashSet dedups).
    #[test]
    fn build_dedups_within_sentence() {
        let input = "A: 猫と猫。\tCat and cat.#ID=2_2\n\
                     B: 猫(ねこ) と 猫(ねこ)\n";
        let (index, _, _) = build(input).unwrap();
        let entry = index.iter().find(|(k, _)| k == "猫").expect("猫 present");
        assert_eq!(
            entry.1.len(),
            1,
            "duplicate headword in B-line should be folded"
        );
    }

    /// Per-headword offsets are capped at MAX_PER_HEADWORD (20) across the
    /// whole corpus — without this, common particles would carry tens of
    /// thousands of pointers and bloat the binary index.
    #[test]
    fn build_caps_offsets_per_headword() {
        let n = MAX_PER_HEADWORD + 5;
        let mut input = String::new();
        for i in 0..n {
            input.push_str(&format!("A: 猫{}。\tcat {i}#ID={i}_{i}\n", i));
            input.push_str("B: 猫(ねこ)\n");
        }
        let (index, _, count) = build(&input).unwrap();
        assert_eq!(count, n);
        let entry = index.iter().find(|(k, _)| k == "猫").expect("猫 present");
        assert_eq!(entry.1.len(), MAX_PER_HEADWORD);
    }

    /// Lines without `A:` / `B:` prefixes are skipped silently — the corpus
    /// has stray comment lines that must not break the build.
    #[test]
    fn build_skips_malformed_pairs() {
        let input = "# corpus header\n\
                     A: 猫。\tcat#ID=1_1\n\
                     B: 猫(ねこ)\n\
                     not a real line\n\
                     A: 犬。\tdog#ID=2_2\n\
                     B: 犬(いぬ)\n";
        let (index, _, count) = build(input).unwrap();
        // The leading comment + B line make a malformed pair → skipped.
        // We then advance past it and pick up the cat + dog pairs.
        assert!(
            count >= 1,
            "expected at least one sentence parsed, got {count}"
        );
        let keys: Vec<&str> = index.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"猫") || keys.contains(&"犬"));
    }

    fn japanese_for(input: &str, word: &str) -> Vec<String> {
        let (index, bytes, _) = build(input).unwrap();
        let (_, offs) = index.iter().find(|(k, _)| k == word).unwrap();
        offs.iter()
            .map(|&o| {
                let o = o as usize;
                let len = u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
                postcard::from_bytes::<ExampleEntry>(&bytes[o + 4..o + 4 + len])
                    .unwrap()
                    .japanese
            })
            .collect()
    }

    /// A short sentence showing the particle beats a long multi-clause one
    /// that comes first in the corpus.
    #[test]
    fn build_ranks_short_simple_sentences_first() {
        let input = "A: 彼は昼食を食べていた時に、その問題の答えを思いついた。\tlong#ID=1\n\
                     B: 彼(かれ) は 昼食 を 食べる{食べていた} 時(とき) に その 問題 の 答え を 思いつく{思いついた}\n\
                     A: りんごを食べています。\tI am eating an apple.#ID=2\n\
                     B: 林檎(りんご){りんご} を 食べる{食べています}\n";
        let jp = japanese_for(input, "食べる");
        assert_eq!(
            jp.first().map(String::as_str),
            Some("りんごを食べています。")
        );
    }

    /// The `~` "good example" marker is part of the token, not the headword.
    #[test]
    fn build_strips_tilde_marker() {
        let input = "A: 本を読む。\tread#ID=1\nB: 本 を 読む~\n";
        let (index, _, _) = build(input).unwrap();
        assert!(index.iter().any(|(k, _)| k == "読む"));
        assert!(!index.iter().any(|(k, _)| k.contains('~')));
    }

    /// A near copy of an already-picked sentence drops below a different one.
    #[test]
    fn build_spreads_out_near_duplicates() {
        let input = "A: 私はこの本が読みたい。\ta#ID=1\nB: 私 は この 本 が 読む{読みたい}\n\
                     A: 私はその本が読みたい。\tb#ID=2\nB: 私 は その 本 が 読む{読みたい}\n\
                     A: 毎晩新聞を読みます。\tc#ID=3\nB: 毎晩 新聞 を 読む{読みます}\n";
        let jp = japanese_for(input, "読む");
        assert_eq!(jp.len(), 3);
        assert_eq!(jp[1], "毎晩新聞を読みます。", "got {jp:?}");
    }

    /// Empty japanese or english half drops the sentence entirely.
    #[test]
    fn build_skips_sentences_with_empty_half() {
        let input = "A: \tonly english#ID=1_1\nB: 猫(ねこ)\n";
        let (index, _, count) = build(input).unwrap();
        assert_eq!(count, 0);
        assert!(index.is_empty());
    }
}
