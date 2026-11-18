//! The grammar dictionary: Yomeru's own `data/grammar.toml`, compiled into
//! `grammar.bin` and shipped like JMdict. Lookups go through
//! [`crate::platform::DictClient::grammar_search`]: the server, the
//! Android app's bundled copy, or the extension's background.

pub use grammar_core::{AnswerCheck, Example, GrammarEntry, Related, Rich, Span, Verdict};

/// Deletes the grammar dictionary earlier versions downloaded and converted
/// on the device (bee-san's, never ours to keep around). Once per device;
/// harmless when there is none.
pub async fn remove_old_download() {
    const DONE_KEY: &str = "yomeru_old_grammar_removed";
    if crate::rt::kv_get::<bool>(DONE_KEY).unwrap_or(false) {
        return;
    }
    match old::delete().await {
        Ok(()) => {
            if let Err(e) = crate::rt::kv_set(DONE_KEY, &true) {
                tracing::warn!("[yomeru] {e}");
            }
        }
        Err(e) => tracing::warn!("[yomeru] removing the old grammar dictionary: {e}"),
    }
}

/// Its own IndexedDB database, `yomeru-grammar`.
#[cfg(target_arch = "wasm32")]
mod old {
    pub(super) async fn delete() -> Result<(), String> {
        let err = |e: idb::Error| format!("{e:?}");
        idb::Factory::new()
            .map_err(err)?
            .delete("yomeru-grammar")
            .map_err(err)?
            .await
            .map_err(err)
    }
}

/// `grammar.bin` in the app's data dir.
#[cfg(not(target_arch = "wasm32"))]
mod old {
    pub(super) async fn delete() -> Result<(), String> {
        let path = crate::rt::data_dir().join("grammar.bin");
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("deleting {}: {e}", path.display())),
        }
    }
}
