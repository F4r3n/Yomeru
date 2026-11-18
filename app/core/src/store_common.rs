//! The card-store API that doesn't depend on the storage backend: row types
//! and the operations built from the backend's primitives. Re-exported from
//! [`crate::idb`] on both targets (IndexedDB on wasm32, SQLite natively), so
//! callers keep writing `crate::idb::promote_card` either way.

use serde::{Deserialize, Serialize};

use crate::idb::{delete_ids_with_tombstones, get_all_cards, get_cards_of, put_all};
use crate::types::{CardDirection, CardItem, CardKind, CardStatus, SrsCard};

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
/// arrived with. Only the sync merge should use this — see `put_all` — and
/// [`ensure_grammar_production_cards`], whose cards must lose to any real one.
pub async fn put_cards_synced(cards: &[SrsCard]) -> Result<(), String> {
    put_all(cards, false).await
}

/// Due word cards, ordered by [`crate::srs::order_due`], one per word
/// ([`crate::srs::hold_siblings`]): the other waits for the next session. Kanji cards have their own session — see
/// [`get_due_kanji_cards`].
pub async fn get_due_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, CardKind::Word, None).await
}

/// Due kanji cards, ordered by [`crate::srs::order_due`].
pub async fn get_due_kanji_cards(now_ms: f64) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, CardKind::Kanji, None).await
}

/// What a Grammar review session takes: recognition cards, production
/// cards, or both mixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrammarMode {
    Mix,
    Recognition,
    Production,
}

impl GrammarMode {
    pub const ALL: [GrammarMode; 3] = [Self::Mix, Self::Recognition, Self::Production];

    /// The one direction the session takes; `None` for both.
    pub fn direction(self) -> Option<CardDirection> {
        match self {
            Self::Mix => None,
            Self::Recognition => Some(CardDirection::Recognition),
            Self::Production => Some(CardDirection::Production),
        }
    }

    /// Whether `card` belongs in a session of this mode.
    pub fn takes(self, card: &SrsCard) -> bool {
        card.is_grammar() && self.direction().is_none_or(|d| card.direction == d)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mix => "mix",
            Self::Recognition => "recognition",
            Self::Production => "production",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mix => "Mix",
            Self::Recognition => "Recognition",
            Self::Production => "Production",
        }
    }
}

/// Due grammar cards of a `mode`'s session, ordered by
/// [`crate::srs::order_due`], one per point: in a mix, a point's other card
/// waits for the next session.
pub async fn get_due_grammar_cards(now_ms: f64, mode: GrammarMode) -> Result<Vec<SrsCard>, String> {
    get_due(now_ms, CardKind::Grammar, mode.direction()).await
}

/// How many cards each [`GrammarMode`]'s session would take now, in
/// [`GrammarMode::ALL`] order, from one read of the store.
pub async fn count_due_grammar(now_ms: f64) -> Result<[usize; 3], String> {
    Ok(grammar_due_counts(get_all_cards().await?, now_ms))
}

fn grammar_due_counts(mut due: Vec<SrsCard>, now_ms: f64) -> [usize; 3] {
    due.retain(|c| c.is_grammar() && is_due(c, now_ms));
    crate::srs::order_due(&mut due);
    GrammarMode::ALL.map(|m| {
        let of_mode: Vec<SrsCard> = due.iter().filter(|c| m.takes(c)).cloned().collect();
        crate::srs::hold_siblings(of_mode).len()
    })
}

fn is_due(c: &SrsCard, now_ms: f64) -> bool {
    matches!(c.status, CardStatus::Active) && c.due_ms <= now_ms
}

async fn get_due(
    now_ms: f64,
    kind: CardKind,
    direction: Option<CardDirection>,
) -> Result<Vec<SrsCard>, String> {
    let mut due = get_all_cards().await?;
    due.retain(|c| {
        c.kind() == kind && direction.is_none_or(|d| c.direction == d) && is_due(c, now_ms)
    });
    crate::srs::order_due(&mut due);
    Ok(crate::srs::hold_siblings(due))
}

/// Gives every grammar point in the deck its production card, in staging,
/// for the points added before production cards existed (or on a build
/// without them). Run when the grammar screens load; returns how many it
/// made.
///
/// A device can't know whether another one already made and reviewed the
/// card, so the new one is written as if synced: not uploaded until changed
/// here, and stamped with the recognition card's `added_ms`, older than any
/// real edit, so the next sync keeps another device's copy over it.
pub async fn ensure_grammar_production_cards() -> Result<usize, String> {
    let all = get_all_cards().await?;
    let missing = missing_production_cards(&all);
    if !missing.is_empty() {
        put_cards_synced(&missing).await?;
    }
    Ok(missing.len())
}

/// The production cards [`ensure_grammar_production_cards`] makes: one per
/// grammar recognition card without one, staged, priority 0.
pub fn missing_production_cards(all: &[SrsCard]) -> Vec<SrsCard> {
    let have: std::collections::HashSet<&str> = all.iter().map(|c| c.id.as_str()).collect();
    all.iter()
        .filter(|c| c.is_grammar() && c.direction == CardDirection::Recognition)
        .filter_map(|rec| {
            let mut c = SrsCard::new(rec.item, CardDirection::Production, rec.added_ms);
            if have.contains(c.id.as_str()) {
                return None;
            }
            c.updated_ms = rec.added_ms;
            Some(c)
        })
        .collect()
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

/// Bumps the priority score of every staging sibling of `item` by 1, up
/// to [`crate::types::MAX_PRIORITY`]. Called when the user re-clicks "Add"
/// on an already-staged word.
pub async fn bump_priority(item: CardItem) -> Result<(), String> {
    let siblings = get_cards_of(item).await?;
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

/// Resets every active or graduated sibling of `item` back to a fresh
/// "New" FSRS state in the active word list — the mechanism by which a
/// graduated card can re-enter Review. Destructive.
pub async fn reset_card(item: CardItem, now_ms: f64) -> Result<(), String> {
    let siblings = get_cards_of(item).await?;
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

/// Puts every graduated card back into Review with its scheduling kept, so
/// each comes up on its own due date. Returns how many came back.
pub async fn reactivate_graduated() -> Result<usize, String> {
    let mut cards = get_all_cards().await?;
    cards.retain(|c| matches!(c.status, CardStatus::Graduated));
    for c in &mut cards {
        c.status = CardStatus::Active;
    }
    put_cards(&cards).await?;
    Ok(cards.len())
}

/// Sends every active or graduated sibling of `item` back to the New
/// Words tab: progress reset, status back to Staging. The inverse of
/// [`promote_card`]; `added_ms` and `priority` are kept. Destructive.
pub async fn demote_card(item: CardItem, now_ms: f64) -> Result<(), String> {
    let siblings = get_cards_of(item).await?;
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

pub async fn promote_card(item: CardItem) -> Result<(), String> {
    let siblings = get_cards_of(item).await?;
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

/// Deletes every card of `item` (a word's two directions, a kanji's or a
/// grammar point's one), with tombstones.
pub async fn delete_card(item: CardItem) -> Result<(), String> {
    delete_ids_with_tombstones(&item.card_ids()).await
}

pub async fn delete_card_by_id(id: &str) -> Result<(), String> {
    delete_ids_with_tombstones(std::slice::from_ref(&id.to_string())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mix_takes_one_card_per_point() {
        let due = |item, d| {
            let mut c = SrsCard::new(item, d, 0.0);
            c.status = CardStatus::Active;
            c
        };
        let cards = vec![
            // Both cards of point 1 due, one of point 2, one of each kind.
            due(CardItem::Grammar(1), CardDirection::Recognition),
            due(CardItem::Grammar(1), CardDirection::Production),
            due(CardItem::Grammar(2), CardDirection::Production),
            due(CardItem::Word(1_000_001), CardDirection::Recognition),
            {
                let mut later = due(CardItem::Grammar(3), CardDirection::Recognition);
                later.due_ms = 5_000.0;
                later
            },
        ];
        // Mix, recognition, production.
        assert_eq!(grammar_due_counts(cards, 1_000.0), [2, 1, 2]);
        for m in GrammarMode::ALL {
            assert_eq!(GrammarMode::parse(m.as_str()), Some(m));
        }
    }

    #[test]
    fn each_grammar_point_gets_one_production_card_older_than_any_edit() {
        let mut rec = SrsCard::new(CardItem::Grammar(7), CardDirection::Recognition, 1_000.0);
        rec.status = CardStatus::Active;
        rec.updated_ms = 9_000.0;
        let done = SrsCard::new_cards(CardItem::Grammar(8), 2_000.0);
        let word = SrsCard::new_cards(CardItem::Word(1_000_001), 3_000.0);
        let all: Vec<SrsCard> = [vec![rec], done, word].concat();

        let made = missing_production_cards(&all);
        assert_eq!(made.len(), 1, "{made:?}");
        let c = &made[0];
        assert_eq!(c.id, "grammar:7:production");
        assert_eq!(c.status, CardStatus::Staging);
        assert_eq!(
            (c.added_ms, c.updated_ms, c.priority),
            (1_000.0, 1_000.0, 0)
        );
        // Run again with it stored: nothing more.
        let all: Vec<SrsCard> = [all, made].concat();
        assert!(missing_production_cards(&all).is_empty());
    }
}
