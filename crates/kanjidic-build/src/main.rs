// Offline build CLI: progress/diagnostic output to stderr is intentional.
#![allow(clippy::print_stderr)]

mod parser;
mod serializer;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "kanjidic-build", about = "Build binary KANJIDIC2 index from XML")]
struct Args {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long, default_value = "extension/data/kanjidic.bin")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    eprintln!("Parsing KANJIDIC2 XML from {:?}...", args.input);
    let entries = parser::parse_kanjidic(&args.input)?;
    eprintln!("Parsed {} entries.", entries.len());
    eprintln!("Serializing to {:?}...", args.output);
    serializer::write_index(&entries, &args.output)?;
    let size = std::fs::metadata(&args.output)?.len();
    eprintln!("Done. Output size: {:.2} MB", size as f64 / 1_048_576.0);
    Ok(())
}
