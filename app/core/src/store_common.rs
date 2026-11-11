//! The card-store API that doesn't depend on the storage backend: row types
//! and the operations built from the backend's primitives. Re-exported from
//! [`crate::idb`] on both targets (IndexedDB on wasm32, SQLite natively), so
//! callers keep writing `crate::idb::promote_card` either way.

use serde::{Deserialize, Serialize};

use crate::idb::{delete_ids_with_tombstones, get_all_cards, get_cards_by_sequence, put_all};
use crate::types::{CardDirection, CardStatus, SrsCard, card_id};

/// A local record that the user deleted a card, and when. The time travels to
/// the server on the next sync so a delete made while offline is ordered by
/// when it actually happened rather than when it was finally uploaded.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tombstone {
    pub id: String,
    pub deleted_at: f64,
}

/// One dirty-set entry: card `id`, changed at stamp `v`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DirtyEntry {
    pub id: String,
    pub v: f64,
}

pub async fn put_card(card: &SrsCard) -> Result<(), String> {
    put_all(std::slice::from_ref(card), true).await
}

pub async fn put_cards(cards: &[SrsCard]) -> Result<(), String> {
    put_all(cards, true).await
}

/// Writes cards adopted from the server verbatim, keeping the `updated_ms` they
/// arrived with. Only the sync merge should use this — see `put_all`.
pub async fn put_cards_synced(cards: &[SrsCard]) -> Result<(), String> {
    put_all(cards, false).await
}

/// Due word cards, ordered by [`crate::srs::order_due`]. Kanji cards have their own session — see
/// [`get_due_kanji_cards`].
pub async fn get_due_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, false).await
}

/// Due kanji cards, ordered by [`crate::srs::order_due`].
pub async fn get_due_kanji_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, true).await
}

async fn get_due(now_ms: f64, kanji: bool) -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| {
        c.is_kanji() == kanji && matches!(c.status, CardStatus::Active) && c.due_ms <= now_ms
    });
    crate::srs::order_due(&mut all);
    Ok(all)
}

pub async fn get_staging_cards() -> Result<Vec<SrsCard>, String> {
    let mut all = get_all_cards().await?;
    all.retain(|c| matches!(c.status, CardStatus::Staging));
    all.sort_by(|a, b| {
        b.priority.cmp(&a.priority).then_with(|| {
            b.added_ms
                .partial_cmp(&a.added_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    Ok(all)
}

/// Bumps the priority score of every staging sibling of `sequence` by 1, up
/// to [`crate::types::MAX_PRIORITY`]. Called when the user re-clicks "Add"
/// on an already-staged word.
pub async fn bump_priority(sequence: u32) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Staging))
        .map(|mut c| {
            c.priority = c.priority.saturating_add(1).min(crate::types::MAX_PRIORITY);
            c
        })
        .collect();
    put_cards(&to_put).await
}

/// Resets every active or graduated sibling of `sequence` back to a fresh
/// "New" FSRS state in the active word list — the mechanism by which a
/// graduated card can re-enter Review. Destructive.
pub async fn reset_card(sequence: u32, now_ms: f64) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Active | CardStatus::Graduated))
        .map(|mut c| {
            c.reset_progression(now_ms);
            c.status = CardStatus::Active;
            c
        })
        .collect();
    put_cards(&to_put).await
}

/// Sends every active or graduated sibling of `sequence` back to the New
/// Words tab: progress reset, status back to Staging. The inverse of
/// [`promote_card`]; `added_ms` and `priority` are kept. Destructive.
pub async fn demote_card(sequence: u32, now_ms: f64) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Active | CardStatus::Graduated))
        .map(|mut c| {
            c.reset_progression(now_ms);
            c.status = CardStatus::Staging;
            c
        })
        .collect();
    put_cards(&to_put).await
}

pub async fn promote_card(sequence: u32) -> Result<(), String> {
    let siblings = get_cards_by_sequence(sequence).await?;
    let to_put: Vec<SrsCard> = siblings
        .into_iter()
        .filter(|c| matches!(c.status, CardStatus::Staging))
        .map(|mut c| {
            c.status = CardStatus::Active;
            c
        })
        .collect();
    put_cards(&to_put).await
}

pub async fn delete_card(sequence: u32) -> Result<(), String> {
    let ids = [
        card_id(sequence, CardDirection::Recognition),
        card_id(sequence, CardDirection::Recall),
    ];
    delete_ids_with_tombstones(&ids).await
}

pub async fn delete_card_by_id(id: &str) -> Result<(), String> {
    delete_ids_with_tombstones(std::slice::from_ref(&id.to_string())).await
}
