//! The answer log, from the app's side: every rating — scheduled review or
//! drill — is queued in IndexedDB's review outbox and uploaded on the next
//! sync. The server keeps the whole log and answers each sync with per-card
//! drill counts, which decide the batch the next drill picks.
//!
//! Counts shown here are the server's as of the last sync plus the drills still
//! waiting in the outbox, so a drill done offline or between syncs is counted
//! straight away. Without a sync account the outbox is never flushed and the
//! counts come from it alone.

use std::collections::HashMap;

use crate::idb::{add_review, get_drill_state};
use crate::platform::Platform;
use crate::srs::{drill_counts_by_sequence, now_ms};
use crate::types::{ReviewKind, ReviewLog, SrsCard};

/// Where drill counts lived before the answer log: per device, in
/// localStorage. Their counts aren't carried over (the log restarts the
/// rotation), so the keys are just cleaned up.
const LEGACY_KEYS: [&str; 2] = ["drill_counts", "kanji_drill_counts"];

/// Sequence → number of drill sessions the word or kanji was answered in.
/// Word and kanji sequences never collide, so one map serves both decks.
/// Empty when nothing is stored or storage is unavailable.
pub async fn load() -> HashMap<u32, u32> {
    for key in LEGACY_KEYS {
        crate::rt::kv_delete(key);
    }
    let (mut per_card, outbox) = get_drill_state().await.unwrap_or_else(|e| {
        tracing::warn!("drill_log: reading drill counts failed: {e}");
        (HashMap::new(), Vec::new())
    });
    for r in outbox.iter().filter(|r| r.kind == ReviewKind::Drill) {
        *per_card.entry(r.card_id.clone()).or_insert(0) += 1;
    }
    drill_counts_by_sequence(per_card.iter().map(|(id, n)| (id.as_str(), *n)))
}

/// The log row for answering `card` with `rating` now. A scheduled review
/// saves it together with the card via [`crate::idb::put_card_logged`], so the
/// log can neither record a review that didn't apply nor miss one that did.
pub fn answer(card: &SrsCard, rating: u8, kind: ReviewKind) -> ReviewLog {
    ReviewLog {
        card_id: card.id.clone(),
        rating,
        kind,
        reviewed_ms: now_ms(),
    }
}

/// Log a drill answer and arm a sync to upload it. A drill changes no card, so
/// the row is all there is to write. Best-effort: a failed write only costs the
/// rotation order.
///
/// The write runs on a task of its own rather than the calling component's, so
/// leaving the page right after the last answer can't cancel it.
pub fn record_drill(platform: &Platform, card: &SrsCard, rating: u8) {
    let row = answer(card, rating, ReviewKind::Drill);
    let platform = platform.clone();
    crate::rt::spawn_detached(async move {
        match add_review(&row).await {
            Ok(()) => platform.settings.schedule_sync(),
            Err(e) => tracing::warn!("drill_log: logging answer to {} failed: {e}", row.card_id),
        }
    });
}
