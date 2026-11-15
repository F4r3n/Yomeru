//! Install, reload and remove a real grammar dictionary through
//! `yomeru_core::grammar`, against files in a throwaway data dir:
//!
//! ```sh
//! YOMERU_DATA_DIR=/tmp/somewhere GRAMMAR_ZIP=path/to/bees-ultimate-grammar-dictionary.zip \
//!     cargo test --release -p yomeru-core --test grammar_real -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::print_stderr)]

use std::time::Instant;

use yomeru_core::grammar;

#[test]
#[ignore = "needs GRAMMAR_ZIP and YOMERU_DATA_DIR"]
fn installs_reloads_and_removes() {
    assert!(
        std::env::var_os("YOMERU_DATA_DIR").is_some(),
        "set YOMERU_DATA_DIR"
    );
    let zip = std::fs::read(std::env::var("GRAMMAR_ZIP").unwrap()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        let t = Instant::now();
        let installed = grammar::install_zip(zip, None).await.unwrap();
        eprintln!(
            "installed {} rev {}: {} entries, {:.1} MB in {:.2?}",
            installed.meta.title,
            installed.meta.revision,
            installed.entries,
            installed.bytes as f64 / 1e6,
            t.elapsed()
        );
        let dict = grammar::current().unwrap();
        assert!(dict.lookup_longest_match("ことにした", 20).is_some());

        // What a fresh start would read back.
        let path = yomeru_core::rt::data_dir().join("grammar.bin");
        let on_disk = std::fs::metadata(&path).unwrap().len();
        assert_eq!(on_disk as usize, installed.bytes);

        let reread = grammar::GrammarDict::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.meta(), &installed.meta);
        assert_eq!(grammar::load().await.unwrap().unwrap(), installed.meta);

        let bad = grammar::install_zip(b"not a zip".to_vec(), None).await;
        assert!(bad.is_err());
        // A failed install leaves the installed one in place.
        assert!(grammar::current().is_some());
        let damaged = grammar::install_zip(
            std::fs::read(std::env::var("GRAMMAR_ZIP").unwrap()).unwrap(),
            Some("0".repeat(64)),
        )
        .await;
        assert!(damaged.unwrap_err().contains("damaged"));

        grammar::remove().await.unwrap();
        assert!(grammar::current().is_none());
        assert!(!path.exists());
        assert_eq!(grammar::load().await.unwrap(), None);
    });
}
