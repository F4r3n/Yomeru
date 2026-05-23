//! Auto-sync shims. The scheduler implementation lives in
//! [`crate::platform::SettingsStore`] — web/android use the
//! [`gloo_timers`] debouncer in `platform.rs`; the extension delegates to
//! `background.ts` via a `BUMP_DB_VERSION` / `SYNC_CARDS` message.
//!
//! These free functions keep existing call sites (every mutation site in
//! `routes/*`) source-stable.

use dioxus::prelude::consume_context;

use crate::platform::Platform;

/// Arms a debounced auto-sync. No-op if the user hasn't configured a
/// server token. Safe to call after every IDB mutation — the debounce
/// coalesces bursts.
pub fn schedule_sync() {
    consume_context::<Platform>().settings.schedule_sync();
}

/// Forces an immediate sync, bypassing the debounce. Used by the
/// "Sync now" button in Settings.
pub async fn sync_now() -> Result<String, String> {
    consume_context::<Platform>().settings.sync_now().await
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    /// Pure logic behind the re-add-during-sync race fix, mirrored from
    /// `platform.rs::do_sync`. Kept here so we have a host-target check on
    /// the filter even if the platform module itself isn't host-buildable.
    fn foreign_deletions(resp_deletions: &[&str], sent: &[&str]) -> Vec<String> {
        let sent: HashSet<&str> = sent.iter().copied().collect();
        resp_deletions
            .iter()
            .filter(|id| !sent.contains(**id))
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn ids_we_sent_are_excluded_from_remote_deletions() {
        let resp = ["猫::recognition", "犬::recognition"];
        let sent = ["猫::recognition"];
        assert_eq!(
            foreign_deletions(&resp, &sent),
            vec!["犬::recognition".to_string()]
        );
    }

    #[test]
    fn empty_sent_means_all_remote_deletions_apply() {
        let resp = ["猫::recognition", "犬::recognition"];
        let sent: [&str; 0] = [];
        assert_eq!(foreign_deletions(&resp, &sent).len(), 2);
    }

    #[test]
    fn empty_resp_means_nothing_to_apply() {
        let resp: [&str; 0] = [];
        let sent = ["猫::recognition"];
        assert!(foreign_deletions(&resp, &sent).is_empty());
    }
}
