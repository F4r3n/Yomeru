//! Reading and writing the card deck as JSON — the Settings tab's
//! export/import buttons, minus the UI.
//!
//! Lives outside `routes/` because none of it is route-specific: it is data
//! marshalling that happens to be triggered from Settings, and keeping it here
//! means the format handling can be tested without standing up a component.
//!
//! Two on-disk formats are read. The current one (`schema >= CARDS_SCHEMA_VERSION`)
//! deserializes straight into [`SrsCard`]; the legacy one keyed cards by headword
//! rather than JMdict `sequence`, so it has to resolve each word through the
//! dictionary first.

use wasm_bindgen::JsCast;

use crate::dict::lookup_many;
use crate::idb::{get_all_cards, put_cards};
use crate::sync::schedule_sync;
use crate::types::{CARDS_SCHEMA_VERSION, SrsCard, SrsCardV1};

/// Triggers a download by creating a Blob URL on a transient <a>. Same pattern
/// the extension uses for JSON export.
pub fn trigger_download(content: &str, filename: &str) -> Result<(), String> {
    use wasm_bindgen::JsValue;
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let blob_parts = js_sys::Array::new();
    blob_parts.push(&JsValue::from_str(content));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/json");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &opts)
        .map_err(|_| "blob create failed".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "url create failed".to_string())?;
    let a = document
        .create_element("a")
        .map_err(|_| "create_element failed".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "anchor cast failed".to_string())?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    web_sys::Url::revoke_object_url(&url).ok();
    Ok(())
}

/// Breakdown of cards that were not imported, by reason.
#[derive(Default, Clone, Copy)]
pub struct Skips {
    /// Id already present in storage.
    existing: usize,
    /// Legacy headword not found in the dictionary (no `sequence` to key by).
    unresolved: usize,
    /// Could not be parsed in the expected format.
    invalid: usize,
}

impl Skips {
    fn total(self) -> usize {
        self.existing + self.unresolved + self.invalid
    }
}

/// Human-readable import summary, e.g.
/// "Imported 3 card(s); skipped 160 (157 already present, 3 not in dictionary)."
pub fn format_import_result(added: usize, skips: Skips) -> String {
    if skips.total() == 0 {
        return format!("Imported {added} card(s).");
    }
    let mut reasons = Vec::new();
    if skips.existing > 0 {
        reasons.push(format!("{} already present", skips.existing));
    }
    if skips.unresolved > 0 {
        reasons.push(format!("{} not in dictionary", skips.unresolved));
    }
    if skips.invalid > 0 {
        reasons.push(format!("{} invalid", skips.invalid));
    }
    format!(
        "Imported {added} card(s); skipped {} ({}).",
        skips.total(),
        reasons.join(", ")
    )
}

pub async fn import_cards_json(text: &str) -> Result<(usize, Skips), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let arr = v
        .get("cards")
        .and_then(|c| c.as_array())
        .ok_or("missing 'cards' array")?;

    let existing = get_all_cards().await.map_err(|e| e.to_string())?;
    let existing_ids: std::collections::HashSet<String> =
        existing.into_iter().map(|c| c.id).collect();

    // Route by the explicit `schema` version. Legacy exports predate this field
    // (or carry a lower number) and keyed cards by the headword string rather
    // than the JMdict `sequence`. Note: the human-facing `version` field cannot
    // discriminate — it's the crate version and is identical across formats.
    let schema = v.get("schema").and_then(|s| s.as_u64()).unwrap_or(1);
    let (to_put, skips) = if schema >= CARDS_SCHEMA_VERSION {
        parse_current_cards(arr, &existing_ids)
    } else {
        let word_seq = resolve_legacy_words(arr).await?;
        parse_legacy_cards(arr, &word_seq, &existing_ids)
    };

    // Both parse paths drop ids already present locally, so everything left is
    // genuinely (re-)entering the collection now. Stamp `added_ms` accordingly:
    // the server treats a card older than a tombstone as a stale replica and
    // refuses it, so an import carrying its original `added_ms` would be
    // rejected, then deleted locally on the next sync — the restored cards
    // would appear and silently vanish. `added_ms` is also FSRS's last-review
    // fallback, so this resets the scheduling baseline for never-reviewed
    // cards; reviewed ones carry `last_review_ms` and are unaffected.
    let now = js_sys::Date::now();
    let to_put: Vec<SrsCard> = to_put
        .into_iter()
        .map(|mut c| {
            c.added_ms = now;
            c
        })
        .collect();

    let added = to_put.len();
    put_cards(&to_put).await.map_err(|e| e.to_string())?;
    if added > 0 {
        schedule_sync();
    }
    Ok((added, skips))
}

/// Parse current-format cards, which deserialize straight into [`SrsCard`].
/// Unparseable entries and ids already in `existing_ids` count as skipped.
fn parse_current_cards(
    cards: &[serde_json::Value],
    existing_ids: &std::collections::HashSet<String>,
) -> (Vec<SrsCard>, Skips) {
    let mut to_put = Vec::new();
    let mut skips = Skips::default();
    for c in cards {
        let Ok(card) = serde_json::from_value::<SrsCard>(c.clone()) else {
            skips.invalid += 1;
            continue;
        };
        if existing_ids.contains(&card.id) {
            skips.existing += 1;
            continue;
        }
        to_put.push(card);
    }
    (to_put, skips)
}

/// Resolve every legacy card's headword to its JMdict `sequence` in one batch.
async fn resolve_legacy_words(
    cards: &[serde_json::Value],
) -> Result<std::collections::HashMap<String, u32>, String> {
    let mut words: Vec<String> = Vec::new();
    for c in cards {
        if let Ok(v1) = serde_json::from_value::<SrsCardV1>(c.clone())
            && !words.contains(&v1.word)
        {
            words.push(v1.word);
        }
    }
    let mut word_seq = std::collections::HashMap::new();
    if !words.is_empty() {
        let results = lookup_many(&words).await.map_err(|e| e.to_string())?;
        for (w, entries) in words.into_iter().zip(results) {
            if let Some(seq) = entries.first().map(|e| e.sequence) {
                word_seq.insert(w, seq);
            }
        }
    }
    Ok(word_seq)
}

/// Parse legacy-format cards into [`SrsCardV1`] and upgrade each to the current
/// [`SrsCard`] using the resolved `word_seq`. Cards whose word is absent from
/// `word_seq` (not in the dictionary), already-present ids, and unparseable
/// entries are tallied by reason. Returns `(to_put, skips)`.
fn parse_legacy_cards(
    cards: &[serde_json::Value],
    word_seq: &std::collections::HashMap<String, u32>,
    existing_ids: &std::collections::HashSet<String>,
) -> (Vec<SrsCard>, Skips) {
    let mut to_put = Vec::new();
    let mut skips = Skips::default();
    for c in cards {
        let Ok(v1) = serde_json::from_value::<SrsCardV1>(c.clone()) else {
            skips.invalid += 1;
            continue;
        };
        let Some(&seq) = word_seq.get(&v1.word) else {
            skips.unresolved += 1;
            continue;
        };
        let card = v1.upgrade(seq);
        if existing_ids.contains(&card.id) {
            skips.existing += 1;
            continue;
        }
        to_put.push(card);
    }
    (to_put, skips)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CardDirection;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};

    /// A new-format card: keyed by numeric `sequence`, id is `{seq}::{dir}`.
    fn new_format_card(sequence: u32, direction: &str) -> serde_json::Value {
        json!({
            "id": format!("{sequence}::{direction}"),
            "sequence": sequence,
            "direction": direction,
            "due_ms": 1.0,
            "stability": 2.0,
            "difficulty": 3.0,
            "reps": 4,
            "lapses": 0,
            "state": "review",
            "last_review_ms": 0.5,
            "added_ms": 0.0,
            "status": "active",
        })
    }

    /// An old-format card: keyed by `word`, no `sequence`, id is `{word}::{dir}`.
    fn old_format_card(word: &str, direction: &str) -> serde_json::Value {
        json!({
            "id": format!("{word}::{direction}"),
            "word": word,
            "direction": direction,
            "due_ms": 1.0,
            "stability": 2.0,
            "difficulty": 3.0,
            "reps": 4,
            "lapses": 0,
            "state": "review",
            "last_review_ms": 0.5,
            "added_ms": 0.0,
            "status": "active",
        })
    }

    // --- current format -----------------------------------------------------

    #[test]
    fn current_card_imports_unchanged() {
        let cards = vec![new_format_card(1001, "recall")];
        let (to_put, skips) = parse_current_cards(&cards, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put.len(), 1);
        assert_eq!(to_put[0].sequence, 1001);
        assert_eq!(to_put[0].id, "1001::recall");
    }

    #[test]
    fn current_existing_ids_are_skipped() {
        let cards = vec![
            new_format_card(1001, "recall"),
            new_format_card(1002, "recall"),
        ];
        let existing = HashSet::from(["1001::recall".to_string()]);
        let (to_put, skips) = parse_current_cards(&cards, &existing);
        assert_eq!(skips.existing, 1);
        assert_eq!(skips.total(), 1);
        assert_eq!(to_put.len(), 1);
        assert_eq!(to_put[0].sequence, 1002);
    }

    #[test]
    fn current_unparseable_card_is_invalid() {
        // Missing the required `sequence` field => unparseable as SrsCard.
        let cards = vec![json!({ "id": "x", "direction": "recall" })];
        let (to_put, skips) = parse_current_cards(&cards, &HashSet::new());
        assert_eq!(skips.invalid, 1);
        assert!(to_put.is_empty());
    }

    #[test]
    fn current_card_without_priority_defaults_to_zero() {
        // `new_format_card` has no `priority` key — exactly what a JSON file
        // exported before this field existed looks like.
        let cards = vec![new_format_card(1001, "recall")];
        let (to_put, skips) = parse_current_cards(&cards, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put[0].priority, 0);
    }

    #[test]
    fn current_card_with_priority_is_preserved() {
        let mut card = new_format_card(1001, "recall");
        card.as_object_mut()
            .unwrap()
            .insert("priority".to_string(), json!(3));
        let (to_put, skips) = parse_current_cards(&[card], &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put[0].priority, 3);
    }

    // --- legacy (v1) format --------------------------------------------------

    #[test]
    fn legacy_card_resolves_and_recanonicalizes_id() {
        let cards = vec![old_format_card("一発", "recall")];
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        let (to_put, skips) = parse_legacy_cards(&cards, &word_seq, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put.len(), 1);
        let card = &to_put[0];
        assert_eq!(card.sequence, 1583460);
        // id is rebuilt from sequence, not the legacy `word::dir` form.
        assert_eq!(card.id, "1583460::recall");
        assert_eq!(card.direction, CardDirection::Recall);
        // Other fields survive the upgrade.
        assert_eq!(card.stability, 2.0);
        assert_eq!(card.last_review_ms, Some(0.5));
        // Legacy exports never had priority — upgrade defaults it to 0.
        assert_eq!(card.priority, 0);
    }

    #[test]
    fn legacy_card_unresolved_when_word_not_in_dictionary() {
        let cards = vec![old_format_card("珍しい単語", "recall")];
        let (to_put, skips) = parse_legacy_cards(&cards, &HashMap::new(), &HashSet::new());
        assert_eq!(skips.unresolved, 1);
        assert!(to_put.is_empty());
    }

    #[test]
    fn legacy_existing_ids_are_skipped() {
        let cards = vec![old_format_card("一発", "recall")];
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        // The *rebuilt* (sequence-keyed) id already exists.
        let existing = HashSet::from(["1583460::recall".to_string()]);
        let (to_put, skips) = parse_legacy_cards(&cards, &word_seq, &existing);
        assert_eq!(skips.existing, 1);
        assert!(to_put.is_empty());
    }

    #[test]
    fn legacy_batch_counts_by_reason() {
        let cards = vec![
            old_format_card("一発", "recall"), // resolved -> imported
            old_format_card("謎", "recall"),   // unresolved -> skipped
            json!({ "not": "a card" }),        // invalid -> skipped
        ];
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        let (to_put, skips) = parse_legacy_cards(&cards, &word_seq, &HashSet::new());
        assert_eq!(to_put.len(), 1);
        assert_eq!(skips.unresolved, 1);
        assert_eq!(skips.invalid, 1);
        assert_eq!(skips.total(), 2);
    }

    #[test]
    fn legacy_last_review_ms_optional() {
        // A v1 card with no last_review_ms still parses (field defaults to None).
        let mut card = old_format_card("一発", "recall");
        card.as_object_mut().unwrap().remove("last_review_ms");
        let word_seq = HashMap::from([("一発".to_string(), 1583460u32)]);
        let (to_put, skips) = parse_legacy_cards(&[card], &word_seq, &HashSet::new());
        assert_eq!(skips.total(), 0);
        assert_eq!(to_put[0].last_review_ms, None);
    }

    // --- result message ------------------------------------------------------

    #[test]
    fn message_all_imported() {
        assert_eq!(
            format_import_result(3, Skips::default()),
            "Imported 3 card(s)."
        );
    }

    #[test]
    fn message_breaks_down_skips() {
        let skips = Skips {
            existing: 157,
            unresolved: 3,
            invalid: 0,
        };
        assert_eq!(
            format_import_result(0, skips),
            "Imported 0 card(s); skipped 160 (157 already present, 3 not in dictionary)."
        );
    }
}
