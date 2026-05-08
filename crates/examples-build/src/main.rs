use anyhow::Result;
use clap::Parser;
use examples_types::ExampleEntry;
use postcard::to_allocvec;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

// Cap stored offsets per headword to keep the index size manageable.
const MAX_PER_HEADWORD: usize = 20;

#[derive(Parser)]
#[command(name = "examples-build", about = "Build binary example-sentence index from Tanaka Corpus")]
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

        let offset = sentences_bytes.len() as u32;
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
                || (0xF900..=0xFAFF).contains(&n)   // CJK compat
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
