// Offline build CLI: progress/diagnostic output to stderr is intentional.
#![allow(clippy::print_stderr)]

mod kanjivg;
mod parser;
mod serializer;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "kanjidic-build",
    about = "Build binary KANJIDIC2 index from XML"
)]
struct Args {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long, default_value = "extension/data/kanjidic.bin")]
    output: PathBuf,
    /// KanjiVG XML (the single-file release) to take each kanji's parts from.
    #[arg(long)]
    kanjivg: Option<PathBuf>,
    /// Also write the merged entries as JSON, for offline tools such as
    /// the kanji-mnemonics skill's `next_kanji.py`.
    #[arg(long)]
    dump_json: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    eprintln!("Parsing KANJIDIC2 XML from {:?}...", args.input);
    let mut entries = parser::parse_kanjidic(&args.input)?;
    eprintln!("Parsed {} entries.", entries.len());
    if let Some(path) = &args.kanjivg {
        eprintln!("Parsing KanjiVG from {:?}...", path);
        let mut parts = kanjivg::parse_kanjivg(path)?;
        let mut with_parts = 0usize;
        for e in &mut entries {
            if let Some(p) = parts.remove(&e.literal) {
                e.parts = p;
                with_parts += 1;
            }
        }
        eprintln!("Added parts to {with_parts} entries.");
    }
    if let Some(path) = &args.dump_json {
        eprintln!("Dumping JSON to {:?}...", path);
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        serde_json::to_writer(file, &entries)?;
    }
    eprintln!("Serializing to {:?}...", args.output);
    serializer::write_index(&entries, &args.output)?;
    let size = std::fs::metadata(&args.output)?.len();
    eprintln!("Done. Output size: {:.2} MB", size as f64 / 1_048_576.0);
    Ok(())
}
