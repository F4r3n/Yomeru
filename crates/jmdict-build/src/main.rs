// Offline build CLI: progress output to stderr is intentional, and the byte
// scanners in `parser` use bounds-guarded indexing on the input XML buffer.
#![allow(clippy::print_stderr, clippy::indexing_slicing)]

// The disambiguation table needs `re_restr`, which only exists on
// `ReadingElement` under the `full` feature. See `disambig`'s module docs.
#[cfg(feature = "full")]
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

    /// Also emit the build-only `(surface, reading) -> ent_seq` table consumed
    /// by examples-generator, at this path (conventionally under target/, since
    /// it is a build intermediate never shipped to the extension).
    ///
    /// Opt-in: the ordinary dictionary build has no use for it, and producing
    /// it correctly requires `--features full`.
    #[arg(long)]
    disambig_output: Option<PathBuf>,
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

    if let Some(disambig_path) = &args.disambig_output {
        #[cfg(feature = "full")]
        {
            eprintln!("Building disambiguation table -> {disambig_path:?}...");
            let table = disambig::build_disambig(&entries);
            disambig::write_disambig(&table, disambig_path)?;
            let dsize = std::fs::metadata(disambig_path)?.len();
            eprintln!(
                "Done. {} (surface, reading) keys, {:.1} MB",
                table.len(),
                dsize as f64 / 1_048_576.0
            );
        }
        #[cfg(not(feature = "full"))]
        {
            let _ = disambig_path;
            anyhow::bail!(
                "--disambig-output requires --features full: without it JMdict's \
                 re_restr data is not parsed, so every reading would be paired with \
                 every kanji form and the table would map real lookups to the wrong \
                 ent_seq. Rebuild with `cargo run -p jmdict-build --features full`."
            );
        }
    }

    Ok(())
}
