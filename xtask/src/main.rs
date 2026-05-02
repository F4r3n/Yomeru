use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

const JMDICT_URL: &str = "http://ftp.edrdg.org/pub/Nihongo/JMdict_e.gz";

#[derive(Parser)]
#[command(name = "xtask")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build both WASM modules (jmdict-wasm and srs-wasm).
    Build {
        #[arg(long, default_value = "release")]
        profile: String,
    },
    /// Download JMdict_e.gz from EDRDG and decompress it.
    DownloadDict {
        /// Directory to save JMdict_e (default: current directory)
        #[arg(short, long)]
        output_dir: Option<PathBuf>,
    },
    /// Build the JMdict binary index from an XML source file.
    BuildDict {
        /// Path to JMdict_e (uncompressed XML)
        #[arg(short, long)]
        input: PathBuf,
        /// Output path (default: extension/data/jmdict.bin)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Build everything: WASM modules + dict index.
    BuildAll {
        /// Path to JMdict_e (uncompressed XML).
        /// If omitted, downloads it automatically first.
        #[arg(short, long)]
        input: Option<PathBuf>,
    },
    /// Build the extension TypeScript/Svelte sources via npm.
    BuildJs,
    /// Run `cargo test` for host crates only (not WASM).
    Test,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let root = project_root();

    match args.cmd {
        Cmd::Build { profile } => {
            build_wasm(&root, "jmdict-wasm", &profile)?;
            build_wasm(&root, "srs-wasm", &profile)?;
        }

        Cmd::DownloadDict { output_dir } => {
            let dir = output_dir.unwrap_or_else(|| root.clone());
            let out = download_jmdict(&dir)?;
            eprintln!("JMdict ready at {}", out.display());
        }

        Cmd::BuildDict { input, output } => {
            let output = output.unwrap_or_else(|| root.join("extension/data/jmdict.bin"));
            build_dict(&root, &input, &output)?;
        }

        Cmd::BuildAll { input } => {
            let xml_path = match input {
                Some(p) => p,
                None => {
                    eprintln!("No --input given, downloading JMdict automatically...");
                    download_jmdict(&root)?
                }
            };
            build_dict(&root, &xml_path, &root.join("extension/data/jmdict.bin"))?;
            build_wasm(&root, "jmdict-wasm", "release")?;
            build_wasm(&root, "srs-wasm", "release")?;
            build_js(&root)?;
        }

        Cmd::BuildJs => {
            build_js(&root)?;
        }

        Cmd::Test => {
            run(Command::new("cargo")
                .args([
                    "test",
                    "--workspace",
                    "--exclude",
                    "jmdict-wasm",
                    "--exclude",
                    "srs-wasm",
                ])
                .current_dir(&root))?;
        }
    }

    Ok(())
}

/// Downloads JMdict_e.gz from EDRDG, decompresses it, returns the path to the XML file.
fn download_jmdict(dir: &Path) -> Result<PathBuf> {
    use flate2::read::GzDecoder;
    use std::io::{Read, Write};

    std::fs::create_dir_all(dir)?;
    let gz_path = dir.join("JMdict_e.gz");
    let xml_path = dir.join("JMdict_e");

    if xml_path.exists() {
        let size = std::fs::metadata(&xml_path)?.len();
        if size > 1_000_000 {
            eprintln!(
                "{} already exists ({:.1} MB), skipping download.",
                xml_path.display(),
                size as f64 / 1_048_576.0
            );
            return Ok(xml_path);
        }
    }

    // Download
    eprintln!("Downloading {} ...", JMDICT_URL);
    let response = ureq::get(JMDICT_URL)
        .call()
        .context("Failed to connect to ftp.edrdg.org")?;

    let mut gz_bytes: Vec<u8> = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut gz_bytes)
        .context("Failed to read response body")?;

    eprintln!(
        "Downloaded {:.1} MB compressed.",
        gz_bytes.len() as f64 / 1_048_576.0
    );

    // Write gz to disk (optional, but lets the user inspect it).
    std::fs::write(&gz_path, &gz_bytes)?;

    // Decompress
    eprintln!("Decompressing...");
    let mut decoder = GzDecoder::new(gz_bytes.as_slice());
    let mut xml_bytes: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut xml_bytes)
        .context("Failed to decompress JMdict_e.gz")?;

    std::fs::File::create(&xml_path)
        .and_then(|mut f| f.write_all(&xml_bytes))
        .context("Failed to write JMdict_e")?;

    eprintln!(
        "Decompressed to {} ({:.1} MB).",
        xml_path.display(),
        xml_bytes.len() as f64 / 1_048_576.0
    );

    // Remove the .gz now that we have the XML.
    let _ = std::fs::remove_file(&gz_path);

    Ok(xml_path)
}

fn find_npm() -> Result<PathBuf> {
    // Prefer npm already on PATH.
    if let Ok(out) = Command::new("which").arg("npm").output() {
        if out.status.success() {
            let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
            if p.exists() {
                return Ok(p);
            }
        }
    }
    bail!("npm not found — make sure Node.js is installed and npm is on PATH");
}

fn build_js(root: &Path) -> Result<()> {
    let npm = find_npm()?;
    let ext_dir = root.join("extension");
    if !ext_dir.join("node_modules").exists() {
        eprintln!("Running npm install in extension/...");
        run(Command::new(&npm).args(["install"]).current_dir(&ext_dir))?;
    }
    run(Command::new(&npm)
        .args(["run", "build"])
        .current_dir(&ext_dir))?;
    eprintln!("Built extension JS/Svelte → extension/{{content,background,options}}/");
    Ok(())
}

fn build_wasm(root: &Path, crate_name: &str, profile: &str) -> Result<()> {
    let out_dir = root.join(format!("extension/_generated/{crate_name}"));
    let crate_dir = root.join(format!("crates/{crate_name}"));

    let profile_flag = if profile == "release" {
        "--release"
    } else {
        "--dev"
    };

    run(Command::new("wasm-pack")
        .args([
            "build",
            crate_dir.to_str().unwrap(),
            "--target",
            "web",
            "--out-dir",
            out_dir.to_str().unwrap(),
            profile_flag,
        ])
        .current_dir(root))?;

    eprintln!("Built {crate_name} → {}", out_dir.display());
    Ok(())
}

fn build_dict(root: &Path, input: &Path, output: &Path) -> Result<()> {
    run(Command::new("cargo")
        .args([
            "run",
            "--package",
            "jmdict-build",
            "--release",
            "--",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .current_dir(root))?;
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status()?;
    if !status.success() {
        bail!("Command failed: {:?}", cmd);
    }
    Ok(())
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should be inside the workspace")
        .to_owned()
}
