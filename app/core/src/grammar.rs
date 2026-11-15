//! The grammar dictionary on this device.
//!
//! Its content belongs to the sources bee-san's dictionary collects, so
//! Yomeru never ships, hosts or syncs it: the user downloads the publisher's
//! ZIP (or imports one), [`grammar_core::convert`] turns it into
//! `grammar.bin` here, and the file stays in this device's storage —
//! IndexedDB in the browser, the app's data dir natively. Nothing in this
//! module talks to the Yomeru server.

use std::sync::{Arc, Mutex, PoisonError};

pub use grammar_core::{
    Block, Example, GrammarDict, GrammarEntry, GrammarMeta, ParagraphKind, Rich, Sense,
    SourceBlock, Span,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::rt;

/// The publisher's ZIP on their GitHub Pages site, which allows
/// cross-origin downloads (GitHub release assets don't).
pub const DEFAULT_ZIP_URL: &str = "https://bee-san.github.io/bees-ultimate-grammar-dictionary/downloads/bees-ultimate-grammar-dictionary.zip";
/// `sha256  file` lines for each ZIP the publisher releases.
pub const DEFAULT_SUMS_URL: &str =
    "https://bee-san.github.io/bees-ultimate-grammar-dictionary/downloads/SHA256SUMS";
pub const DEFAULT_HOME_URL: &str = "https://github.com/bee-san/bees-ultimate-grammar-dictionary";

/// The loaded dictionary. `None` until [`load`] ran; `Some(None)` when it
/// ran and nothing is installed. A static Mutex rather than a thread-local:
/// Dioxus mobile runs the app on several threads.
static CURRENT: Mutex<Option<Option<Arc<GrammarDict>>>> = Mutex::new(None);

fn set_current(dict: Option<Arc<GrammarDict>>) {
    *CURRENT.lock().unwrap_or_else(PoisonError::into_inner) = Some(dict);
}

/// The loaded dictionary, if [`load`] ran and one is installed.
pub fn current() -> Option<Arc<GrammarDict>> {
    CURRENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .flatten()
}

/// Loads the installed dictionary from storage, once; later calls return
/// what the first found. `Ok(None)` when none is installed. A stored file
/// this build can't read (an older format) is an error asking to import
/// again, and stays stored until then.
pub async fn load() -> Result<Option<GrammarMeta>, String> {
    if let Some(loaded) = CURRENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        return Ok(loaded.map(|d| d.meta().clone()));
    }
    let Some(bytes) = store::read().await? else {
        set_current(None);
        return Ok(None);
    };
    let dict = blocking(move || GrammarDict::from_bytes(&bytes).map_err(|e| e.to_string())).await?;
    let meta = dict.meta().clone();
    set_current(Some(Arc::new(dict)));
    Ok(Some(meta))
}

/// What an install produced, for the settings screen.
#[derive(Debug, Clone)]
pub struct Installed {
    pub meta: GrammarMeta,
    pub entries: usize,
    /// Size of the stored `grammar.bin`.
    pub bytes: usize,
}

/// Converts a Yomitan grammar ZIP, stores the result and makes it current.
/// With `sha256` (lowercase hex), a ZIP with another digest is refused
/// before conversion.
pub async fn install_zip(zip: Vec<u8>, sha256: Option<String>) -> Result<Installed, String> {
    // Give the UI a frame to show its "converting" state: in the browser the
    // conversion below runs on this thread.
    rt::sleep_ms(0).await;
    let converted = blocking(move || {
        if let Some(expected) = sha256 {
            let actual = hex(&Sha256::digest(&zip));
            if actual != expected {
                return Err(format!(
                    "the download is damaged (SHA-256 {actual}, expected {expected}). Try again."
                ));
            }
        }
        let converted = grammar_core::convert(&zip).map_err(|e| format!("{e:#}"))?;
        drop(zip);
        let dict = GrammarDict::from_bytes(&converted.bytes).map_err(|e| e.to_string())?;
        Ok((converted, dict))
    })
    .await?;
    let (converted, dict) = converted;
    let installed = Installed {
        meta: converted.meta,
        entries: converted.entries,
        bytes: converted.bytes.len(),
    };
    store::write(converted.bytes).await?;
    set_current(Some(Arc::new(dict)));
    Ok(installed)
}

/// Downloads the publisher's ZIP and the digest their SHA256SUMS gives it,
/// to hand to [`install_zip`].
pub async fn download_zip() -> Result<(Vec<u8>, String), String> {
    let sums = rt::get_text(DEFAULT_SUMS_URL).await?;
    let name = DEFAULT_ZIP_URL.rsplit('/').next().unwrap_or_default();
    let expected = digest_for(&sums, name)
        .ok_or_else(|| format!("the publisher's SHA256SUMS doesn't list {name}"))?;
    let zip = rt::get_bytes(DEFAULT_ZIP_URL).await?;
    Ok((zip, expected))
}

/// The digest `SHA256SUMS` gives for `file`, lowercased.
fn digest_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        // `sha256sum -b` marks binary files with a leading '*'.
        let name = name.trim().trim_start_matches('*');
        (name == file && digest.len() == 64).then(|| digest.to_ascii_lowercase())
    })
}

/// The revision the publisher now offers, when it differs from the
/// installed one. `Ok(None)` when up to date, or when nothing installed
/// says where to look.
pub async fn check_update() -> Result<Option<String>, String> {
    #[derive(Deserialize)]
    struct Index {
        revision: String,
    }
    let Some(meta) = load().await? else {
        return Ok(None);
    };
    if meta.index_url.is_empty() {
        return Ok(None);
    }
    let text = rt::get_text(&meta.index_url).await?;
    let index: Index = serde_json::from_str(&text).map_err(|e| format!("bad index.json: {e}"))?;
    Ok((index.revision != meta.revision).then_some(index.revision))
}

/// Deletes the stored dictionary.
pub async fn remove() -> Result<(), String> {
    store::delete().await?;
    set_current(None);
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Runs CPU-heavy work off the async runtime natively; in the browser there
/// is one thread, so it just runs.
#[cfg(not(target_arch = "wasm32"))]
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("grammar task failed: {e}"))?
}

#[cfg(target_arch = "wasm32")]
async fn blocking<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    f()
}

// ── Storage ──────────────────────────────────────────────────────────

/// IndexedDB, in a database of its own so the card database's versioning
/// never has to know about it. The extension's pages and background share
/// the extension's origin, so its background script reads the same file.
#[cfg(target_arch = "wasm32")]
mod store {
    use idb::{Database, DatabaseEvent, Factory, ObjectStoreParams, Query, TransactionMode};
    use js_sys::Uint8Array;
    use wasm_bindgen::{JsCast, JsValue};

    const DB_NAME: &str = "yomeru-grammar";
    const DB_VERSION: u32 = 1;
    const STORE: &str = "files";
    const KEY: &str = "grammar.bin";

    fn err(e: impl std::fmt::Debug) -> String {
        format!("grammar storage: {e:?}")
    }

    async fn open() -> Result<Database, String> {
        let mut req = Factory::new()
            .map_err(err)?
            .open(DB_NAME, Some(DB_VERSION))
            .map_err(err)?;
        req.on_upgrade_needed(|event| {
            let Ok(db) = event.database() else {
                return;
            };
            if !db.store_names().iter().any(|n| n == STORE)
                && let Err(e) = db.create_object_store(STORE, ObjectStoreParams::new())
            {
                tracing::error!("grammar idb upgrade: {e:?}");
            }
        });
        req.await.map_err(err)
    }

    pub(super) async fn read() -> Result<Option<Vec<u8>>, String> {
        let db = open().await?;
        let tx = db
            .transaction(&[STORE], TransactionMode::ReadOnly)
            .map_err(err)?;
        let v = tx
            .object_store(STORE)
            .map_err(err)?
            .get(Query::Key(JsValue::from_str(KEY)))
            .map_err(err)?
            .await
            .map_err(err)?;
        Ok(v.and_then(|v| v.dyn_into::<Uint8Array>().ok())
            .map(|a| a.to_vec()))
    }

    pub(super) async fn write(bytes: Vec<u8>) -> Result<(), String> {
        let db = open().await?;
        let tx = db
            .transaction(&[STORE], TransactionMode::ReadWrite)
            .map_err(err)?;
        let value = Uint8Array::from(bytes.as_slice());
        drop(bytes);
        tx.object_store(STORE)
            .map_err(err)?
            .put(&value, Some(&JsValue::from_str(KEY)))
            .map_err(err)?
            .await
            .map_err(err)?;
        tx.commit().map_err(err)?.await.map_err(err)?;
        Ok(())
    }

    pub(super) async fn delete() -> Result<(), String> {
        let db = open().await?;
        let tx = db
            .transaction(&[STORE], TransactionMode::ReadWrite)
            .map_err(err)?;
        tx.object_store(STORE)
            .map_err(err)?
            .delete(Query::Key(JsValue::from_str(KEY)))
            .map_err(err)?
            .await
            .map_err(err)?;
        tx.commit().map_err(err)?.await.map_err(err)?;
        Ok(())
    }
}

/// A file in the app's data dir, replaced through a temp file and a rename
/// so a crash can't leave half of one.
#[cfg(not(target_arch = "wasm32"))]
mod store {
    use std::path::PathBuf;

    fn path() -> PathBuf {
        crate::rt::data_dir().join("grammar.bin")
    }

    pub(super) async fn read() -> Result<Option<Vec<u8>>, String> {
        super::blocking(|| match std::fs::read(path()) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("reading {}: {e}", path().display())),
        })
        .await
    }

    pub(super) async fn write(bytes: Vec<u8>) -> Result<(), String> {
        super::blocking(move || {
            let tmp = path().with_extension("bin.tmp");
            std::fs::write(&tmp, bytes).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
            std::fs::rename(&tmp, path()).map_err(|e| format!("saving {}: {e}", path().display()))
        })
        .await
    }

    pub(super) async fn delete() -> Result<(), String> {
        super::blocking(|| match std::fs::remove_file(path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("deleting {}: {e}", path().display())),
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_digest_of_a_file_in_sha256sums() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = format!("{a}  other.zip\n{b} *grammar.zip\n");
        assert_eq!(digest_for(&sums, "grammar.zip"), Some("b".repeat(64)));
        assert_eq!(digest_for(&sums, "other.zip"), Some(a));
        assert_eq!(digest_for(&sums, "missing.zip"), None);
        assert_eq!(digest_for("short  grammar.zip", "grammar.zip"), None);
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xab]), "000fab");
    }
}
