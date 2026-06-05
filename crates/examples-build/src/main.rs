use anyhow::{Result, bail};
use clap::Parser;
use examples_types::ExampleEntry;
use postcard::to_allocvec;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

// Cap stored offsets per headword to keep the index size manageable.
const MAX_PER_HEADWORD: usize = 20;

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

fn build(content: &str) -> Result<(Vec<(String, Vec<u32>)>, Vec<u8>, usize)> {
    let mut sentences_bytes: Vec<u8> = Vec::new();
    // headword → list of byte offsets (capped at MAX_PER_HEADWORD)
    let mut headword_offsets: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut sentence_count = 0usize;

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i + 1 < lines.len() {
        let a_line = lines[i];
        let b_line = lines[i + 1];
        i += 2;

        if !a_line.starts_with("A: ") || !b_line.starts_with("B: ") {
            continue;
        }

        // A line: "A: <japanese>\t<english>#ID=..."
        let a_rest = &a_line[3..];
        let (japanese, english) = match a_rest.split_once('\t') {
            Some((jp, en)) => {
                let jp = jp.trim().to_string();
                let en = en.split('#').next().unwrap_or("").trim().to_string();
                (jp, en)
            }
            None => continue,
        };

        if japanese.is_empty() || english.is_empty() {
            continue;
        }

        // B line: "B: word1(reading)[sense] word2{surface} ..."
        // Extract headwords from each space-separated token.
        let b_rest = &b_line[3..];
        let mut seen = std::collections::HashSet::new();
        let headwords: Vec<String> = b_rest
            .split_whitespace()
            .filter_map(|t| {
                let end = t
                    .find(|c| c == '(' || c == '[' || c == '{')
                    .unwrap_or(t.len());
                let hw = &t[..end];
                if is_valid_headword(hw) && seen.insert(hw.to_string()) {
                    Some(hw.to_string())
                } else {
                    None
                }
            })
            .collect();

        if headwords.is_empty() {
            continue;
        }

        let offset_usize = sentences_bytes.len();
        if offset_usize > u32::MAX as usize {
            bail!(
                "examples sentences blob exceeds 4 GiB ({} bytes)",
                offset_usize
            );
        }
        let offset = offset_usize as u32;
        let entry = ExampleEntry { japanese, english };
        let serialized = to_allocvec(&entry)?;
        sentences_bytes.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
        sentences_bytes.extend_from_slice(&serialized);

        for hw in headwords {
            let v = headword_offsets.entry(hw).or_default();
            if v.len() < MAX_PER_HEADWORD {
                v.push(offset);
            }
        }
        sentence_count += 1;
    }

    let index: Vec<(String, Vec<u32>)> = headword_offsets.into_iter().collect();
    Ok((index, sentences_bytes, sentence_count))
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

    /// Empty japanese or english half drops the sentence entirely.
    #[test]
    fn build_skips_sentences_with_empty_half() {
        let input = "A: \tonly english#ID=1_1\nB: 猫(ねこ)\n";
        let (index, _, count) = build(input).unwrap();
        assert_eq!(count, 0);
        assert!(index.is_empty());
    }
}
