use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

// Upstream URLs + decompressed filenames live in one file so the
// server Dockerfile can pre-fetch the inputs in its own cacheable
// layer (it sources the same file as a shell env file).
const DICT_SOURCES_ENV: &str = include_str!("../dict-sources.env");

fn source(key: &str) -> &'static str {
    DICT_SOURCES_ENV
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=').map(|(k, v)| (k.trim(), v.trim()))
        })
        .find_map(|(k, v)| (k == key).then_some(v))
        .unwrap_or_else(|| panic!("dict-sources.env missing key `{key}`"))
}

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
    /// Build only the three binary dict indexes (JMdict, KANJIDIC2,
    /// examples). Downloads inputs if missing. Used by the
    /// `yomeru-dicts` Docker builder service, which has neither
    /// wasm-pack nor npm available.
    BuildDicts {
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
            build_extension_wasm(&root, &profile)?;
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
            build_dicts(&root, input)?;
            build_wasm(&root, "jmdict-wasm", "release")?;
            build_wasm(&root, "srs-wasm", "release")?;
            build_wasm(&root, "kanjidic-wasm", "release")?;
            build_wasm(&root, "examples-wasm", "release")?;
            build_extension_wasm(&root, "release")?;
            build_js(&root)?;
        }

        Cmd::BuildDicts { input } => {
            build_dicts(&root, input)?;
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
                    "--exclude",
                    "yomeru-android",
                    "--exclude",
                    "yomeru-extension",
                ])
                .current_dir(&root))?;
        }
    }

    Ok(())
}

/// Downloads JMdict_e.gz from EDRDG, decompresses it, returns the path to the XML file.
fn download_jmdict(dir: &Path) -> Result<PathBuf> {
    download_source(dir, "JMDICT")
}

/// Fetch a gzipped upstream input named by the `<NAME>_URL` /
/// `<NAME>_OUT` keys in `dict-sources.env`. Skips the network call
/// if the decompressed file already exists at `<dir>/<OUT>`.
fn download_source(dir: &Path, name: &str) -> Result<PathBuf> {
    use flate2::read::GzDecoder;
    use std::io::{Read, Write};

    let url = source(&format!("{name}_URL"));
    let out_name = source(&format!("{name}_OUT"));

    std::fs::create_dir_all(dir)?;
    let out_path = dir.join(out_name);

    if let Ok(meta) = std::fs::metadata(&out_path) {
        if meta.len() > 100_000 {
            eprintln!(
                "{} already exists ({:.1} MB), skipping download.",
                out_path.display(),
                meta.len() as f64 / 1_048_576.0
            );
            return Ok(out_path);
        }
    }

    eprintln!("Downloading {} ...", url);
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("Failed to GET {url}"))?;

    let mut gz_bytes: Vec<u8> = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut gz_bytes)
        .context("Failed to read response body")?;

    eprintln!(
        "Downloaded {:.1} MB compressed.",
        gz_bytes.len() as f64 / 1_048_576.0
    );

    let mut decoder = GzDecoder::new(gz_bytes.as_slice());
    let mut decompressed: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .with_context(|| format!("Failed to decompress {name}"))?;

    std::fs::File::create(&out_path)
        .and_then(|mut f| f.write_all(&decompressed))
        .with_context(|| format!("Failed to write {}", out_path.display()))?;

    eprintln!(
        "Decompressed to {} ({:.1} MB).",
        out_path.display(),
        decompressed.len() as f64 / 1_048_576.0
    );

    Ok(out_path)
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
        "options-dx-loader.js",
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
        "_generated/yomeru-extension/yomeru_extension.js",
        "_generated/yomeru-extension/yomeru_extension_bg.wasm",
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

    // The Dioxus app's wasm-bindgen JS statically imports inline snippets from
    // a `snippets/` subdir (one per crate that uses `inline_js`/`eval`). Without
    // them the ESM module fails to load and the options page hangs on "Loading…"
    // — even though it works under `web-ext run`, where the snippets live in the
    // source tree. Copy the whole tree.
    let snippets_src = ext.join("_generated/yomeru-extension/snippets");
    if snippets_src.exists() {
        copy_dir(
            &snippets_src,
            &release.join("_generated/yomeru-extension/snippets"),
        )?;
        eprintln!("  _generated/yomeru-extension/snippets/");
    } else {
        bail!(
            "Missing required dir: {} — run build-all first",
            snippets_src.display()
        );
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

fn build_extension_wasm(root: &Path, profile: &str) -> Result<()> {
    let out_dir = root.join("extension/_generated/yomeru-extension");
    let crate_dir = root.join("app/extension");
    let profile_flag = if profile == "release" { "--release" } else { "--dev" };
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
    eprintln!("Built yomeru-extension → {}", out_dir.display());
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

fn download_kanjidic(dir: &Path) -> Result<PathBuf> {
    download_source(dir, "KANJIDIC")
}

/// Build all three binary dict indexes into `extension/data/`.
/// Downloads missing inputs first.
fn build_dicts(root: &Path, input: Option<PathBuf>) -> Result<()> {
    let xml_path = match input {
        Some(p) => p,
        None => {
            eprintln!("No --input given, downloading JMdict automatically...");
            download_jmdict(root)?
        }
    };
    build_dict(root, &xml_path, &root.join("extension/data/jmdict.bin"))?;
    eprintln!("Downloading KANJIDIC2...");
    let kanjidic_xml = download_kanjidic(root)?;
    build_kanjidic(root, &kanjidic_xml, &root.join("extension/data/kanjidic.bin"))?;
    eprintln!("Downloading example sentences...");
    let examples_utf = download_examples(root)?;
    build_examples(root, &examples_utf, &root.join("extension/data/examples.bin"))?;
    Ok(())
}

fn download_examples(dir: &Path) -> Result<PathBuf> {
    download_source(dir, "EXAMPLES")
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
