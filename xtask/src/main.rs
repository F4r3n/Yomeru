use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

const JMDICT_URL: &str = "http://ftp.edrdg.org/pub/Nihongo/JMdict_e.gz";
const KANJIDIC_URL: &str = "http://ftp.edrdg.org/pub/Nihongo/kanjidic2.xml.gz";
const EXAMPLES_URL: &str = "http://ftp.edrdg.org/pub/Nihongo/examples.utf.gz";

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
    /// Download kanjidic2.xml.gz from EDRDG and decompress it.
    DownloadKanjidic {
        #[arg(short, long)]
        output_dir: Option<PathBuf>,
    },
    /// Build the KANJIDIC2 binary index from an XML source file.
    BuildKanjidic {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Download examples.utf.gz from EDRDG and decompress it.
    DownloadExamples {
        #[arg(short, long)]
        output_dir: Option<PathBuf>,
    },
    /// Build the examples binary index from examples.utf.
    BuildExamples {
        #[arg(short, long)]
        input: PathBuf,
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
    /// Package the extension into release/ and japanese-reader.zip.
    Package,
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
            build_wasm(&root, "kanjidic-wasm", &profile)?;
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

        Cmd::DownloadKanjidic { output_dir } => {
            let dir = output_dir.unwrap_or_else(|| root.clone());
            let out = download_kanjidic(&dir)?;
            eprintln!("KANJIDIC2 ready at {}", out.display());
        }

        Cmd::BuildKanjidic { input, output } => {
            let output = output.unwrap_or_else(|| root.join("extension/data/kanjidic.bin"));
            build_kanjidic(&root, &input, &output)?;
        }

        Cmd::DownloadExamples { output_dir } => {
            let dir = output_dir.unwrap_or_else(|| root.clone());
            let out = download_examples(&dir)?;
            eprintln!("examples.utf ready at {}", out.display());
        }

        Cmd::BuildExamples { input, output } => {
            let output = output.unwrap_or_else(|| root.join("extension/data/examples.bin"));
            build_examples(&root, &input, &output)?;
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
            eprintln!("Downloading KANJIDIC2...");
            let kanjidic_xml = download_kanjidic(&root)?;
            build_kanjidic(
                &root,
                &kanjidic_xml,
                &root.join("extension/data/kanjidic.bin"),
            )?;
            eprintln!("Downloading example sentences...");
            let examples_utf = download_examples(&root)?;
            build_examples(&root, &examples_utf, &root.join("extension/data/examples.bin"))?;
            build_wasm(&root, "jmdict-wasm", "release")?;
            build_wasm(&root, "srs-wasm", "release")?;
            build_wasm(&root, "kanjidic-wasm", "release")?;
            build_wasm(&root, "examples-wasm", "release")?;
            build_js(&root)?;
        }

        Cmd::BuildJs => {
            build_js(&root)?;
        }

        Cmd::Package => {
            package(&root)?;
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
                    "--exclude",
                    "kanjidic-wasm",
                    "--exclude",
                    "examples-wasm",
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

fn package(root: &Path) -> Result<()> {
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    let ext = root.join("extension");
    let release = root.join("release");

    if release.exists() {
        std::fs::remove_dir_all(&release)?;
    }
    std::fs::create_dir_all(&release)?;

    // Static files required by the manifest.
    let static_files = [
        "manifest.json",
        "options.html",
        "_generated/jmdict-wasm/jmdict_wasm.js",
        "_generated/jmdict-wasm/jmdict_wasm_bg.wasm",
        "_generated/srs-wasm/srs_wasm.js",
        "_generated/srs-wasm/srs_wasm_bg.wasm",
        "_generated/kanjidic-wasm/kanjidic_wasm.js",
        "_generated/kanjidic-wasm/kanjidic_wasm_bg.wasm",
        "data/jmdict.bin",
        "data/kanjidic.bin",
        "_generated/examples-wasm/examples_wasm.js",
        "_generated/examples-wasm/examples_wasm_bg.wasm",
        "data/examples.bin",
    ];

    for rel in &static_files {
        let src = ext.join(rel);
        if !src.exists() {
            bail!(
                "Missing required file: {} — run build-all first",
                src.display()
            );
        }
        let dest = release.join(rel);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        std::fs::copy(&src, &dest)?;
        eprintln!("  {rel}");
    }

    // Icons are optional (may not exist during development).
    let icons_src = ext.join("icons");
    if icons_src.exists() {
        copy_dir(&icons_src, &release.join("icons"))?;
        eprintln!("  icons/");
    }

    // Copy the entire dist/ tree (content.js, background.js, options.js + any chunks).
    let dist_src = ext.join("dist");
    if !dist_src.exists() {
        bail!("extension/dist/ not found — run build-js first");
    }
    copy_dir(&dist_src, &release.join("dist"))?;
    eprintln!("  dist/  ({} files)", count_files(&release.join("dist")));

    // Create the zip.
    let zip_path = root.join("yomeru.zip");
    let zip_file = std::fs::File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip_add_dir(&release, &release, &mut zip, opts)?;
    zip.finish()?;

    eprintln!("\nRelease directory : {}", release.display());
    eprintln!("Release zip       : {}", zip_path.display());
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dest.join(entry.file_name());
        if ty.is_file() {
            std::fs::copy(entry.path(), dest_path)?;
        } else if ty.is_dir() {
            copy_dir(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |rd| {
        rd.filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .count()
    })
}

fn zip_add_dir(
    dir: &Path,
    base: &Path,
    zip: &mut zip::ZipWriter<std::fs::File>,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    use std::io::Write;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(base)?
            .to_str()
            .unwrap()
            .replace('\\', "/");
        if path.is_file() {
            zip.start_file(&rel, opts)?;
            zip.write_all(&std::fs::read(&path)?)?;
        } else if path.is_dir() {
            zip_add_dir(&path, base, zip, opts)?;
        }
    }
    Ok(())
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

/// Downloads kanjidic2.xml.gz from EDRDG, decompresses it, returns the path to the XML file.
fn download_kanjidic(dir: &Path) -> Result<PathBuf> {
    use flate2::read::GzDecoder;
    use std::io::{Read, Write};

    std::fs::create_dir_all(dir)?;
    let gz_path = dir.join("kanjidic2.xml.gz");
    let xml_path = dir.join("kanjidic2");

    if xml_path.exists() {
        let size = std::fs::metadata(&xml_path)?.len();
        if size > 100_000 {
            eprintln!(
                "{} already exists ({:.1} MB), skipping download.",
                xml_path.display(),
                size as f64 / 1_048_576.0
            );
            return Ok(xml_path);
        }
    }

    eprintln!("Downloading {} ...", KANJIDIC_URL);
    let response = ureq::get(KANJIDIC_URL)
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

    std::fs::write(&gz_path, &gz_bytes)?;

    eprintln!("Decompressing...");
    let mut decoder = GzDecoder::new(gz_bytes.as_slice());
    let mut xml_bytes: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut xml_bytes)
        .context("Failed to decompress kanjidic2.xml.gz")?;

    std::fs::File::create(&xml_path)
        .and_then(|mut f| f.write_all(&xml_bytes))
        .context("Failed to write kanjidic2")?;

    eprintln!(
        "Decompressed to {} ({:.1} MB).",
        xml_path.display(),
        xml_bytes.len() as f64 / 1_048_576.0
    );

    let _ = std::fs::remove_file(&gz_path);

    Ok(xml_path)
}

fn download_examples(dir: &Path) -> Result<PathBuf> {
    use flate2::read::GzDecoder;
    use std::io::{Read, Write};

    std::fs::create_dir_all(dir)?;
    let gz_path = dir.join("examples.utf.gz");
    let out_path = dir.join("examples.utf");

    if out_path.exists() {
        let size = std::fs::metadata(&out_path)?.len();
        if size > 1_000_000 {
            eprintln!(
                "{} already exists ({:.1} MB), skipping download.",
                out_path.display(),
                size as f64 / 1_048_576.0
            );
            return Ok(out_path);
        }
    }

    eprintln!("Downloading {} ...", EXAMPLES_URL);
    let response = ureq::get(EXAMPLES_URL)
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

    std::fs::write(&gz_path, &gz_bytes)?;

    eprintln!("Decompressing...");
    let mut decoder = GzDecoder::new(gz_bytes.as_slice());
    let mut bytes: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut bytes)
        .context("Failed to decompress examples.utf.gz")?;

    std::fs::File::create(&out_path)
        .and_then(|mut f| f.write_all(&bytes))
        .context("Failed to write examples.utf")?;

    eprintln!(
        "Decompressed to {} ({:.1} MB).",
        out_path.display(),
        bytes.len() as f64 / 1_048_576.0
    );

    let _ = std::fs::remove_file(&gz_path);
    Ok(out_path)
}

fn build_examples(root: &Path, input: &Path, output: &Path) -> Result<()> {
    run(Command::new("cargo")
        .args([
            "run",
            "--package",
            "examples-build",
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

fn build_kanjidic(root: &Path, input: &Path, output: &Path) -> Result<()> {
    run(Command::new("cargo")
        .args([
            "run",
            "--package",
            "kanjidic-build",
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
