// Offline build CLI: progress output to stderr is intentional, and the byte
// scanners in `parser` use bounds-guarded indexing on the input XML buffer.
#![allow(clippy::print_stderr, clippy::indexing_slicing)]

mod disambig;
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

    /// Output path for the build-only `(surface, reading) -> ent_seq` table
    /// consumed by examples-generator. Lives under target/ since it is a build
    /// intermediate — never shipped to the extension.
    #[arg(long, default_value = "target/disambig.bin")]
    disambig_output: PathBuf,
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

    eprintln!("Building disambiguation table -> {:?}...", args.disambig_output);
    let table = disambig::build_disambig(&entries);
    disambig::write_disambig(&table, &args.disambig_output)?;
    let dsize = std::fs::metadata(&args.disambig_output)?.len();
    eprintln!(
        "Done. {} (surface, reading) keys, {:.1} MB",
        table.len(),
        dsize as f64 / 1_048_576.0
    );

    Ok(())
}
