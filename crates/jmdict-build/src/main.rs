// Offline build CLI: progress output to stderr is intentional, and the byte
// scanners in `parser` use bounds-guarded indexing on the input XML buffer.
#![allow(clippy::print_stderr, clippy::indexing_slicing)]

mod indexer;
mod parser;
mod serializer;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "jmdict-build", about = "Build binary JMdict index from XML")]
struct Args {
    /// Path to JMdict_e.xml (or JMdict.xml)
    #[arg(short, long)]
    input: PathBuf,

    /// Output path for the binary index
    #[arg(short, long, default_value = "extension/data/jmdict.bin")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    eprintln!("Parsing JMdict XML from {:?}...", args.input);
    let entries = parser::parse_jmdict(&args.input)?;
    eprintln!("Parsed {} entries.", entries.len());

    eprintln!("Building index...");
    let index = indexer::build_index(&entries)?;

    eprintln!("Serializing to {:?}...", args.output);
    serializer::write_index(&index, &args.output)?;

    let size = std::fs::metadata(&args.output)?.len();
    eprintln!("Done. Output size: {:.1} MB", size as f64 / 1_048_576.0);

    Ok(())
}
