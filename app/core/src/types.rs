use serde::{Deserialize, Serialize};
use srs_core::CardState;

pub const MS_PER_DAY: f64 = 86_400_000.0;

/// Schema version of the card export/import format. Bump whenever the on-disk
/// [`SrsCard`] shape changes. Exports without this field are the legacy v1
/// format ([`SrsCardV1`], keyed by headword); schema 2 ([`SrsCardV2`]) keyed
/// cards by a `u32` `sequence`; schema 3 by `kind` + `item`.
pub const CARDS_SCHEMA_VERSION: u64 = 3;

/// Ceiling on [`SrsCard::priority`] — re-clicking "Add" on an already-staged
/// word bumps it, but stops moving the needle past this.
pub const MAX_PRIORITY: u32 = 10;

/// Which deck, and which Review tab, a card belongs to. On the wire as the
/// card's `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardKind {
    Word,
    Kanji,
    Grammar,
}

impl CardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Kanji => "kanji",
            Self::Grammar => "grammar",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "word" => Some(Self::Word),
            "kanji" => Some(Self::Kanji),
            "grammar" => Some(Self::Grammar),
            _ => None,
        }
    }
}

/// What a card reviews. On the wire as two fields, `kind` and `item` (text):
/// `"word"` + the JMdict `ent_seq`, `"kanji"` + the kanji itself, `"grammar"`
/// + the entry's `seq` from `grammar.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CardItem {
    /// A JMdict entry, by `ent_seq`. Its headword and reading are looked up on
    /// demand and not stored, so a JMdict edit propagates.
    Word(u32),
    Kanji(char),
    /// A grammar point, by its `seq`: issued once from `grammar.toml`'s
    /// `next_seq` and never reused, unlike its slug.
    Grammar(u32),
}

impl CardItem {
    pub fn kind(self) -> CardKind {
        match self {
            Self::Word(_) => CardKind::Word,
            Self::Kanji(_) => CardKind::Kanji,
            Self::Grammar(_) => CardKind::Grammar,
        }
    }

    /// The `item` field: what identifies the item within its kind.
    pub fn key(self) -> String {
        match self {
            Self::Word(n) | Self::Grammar(n) => n.to_string(),
            Self::Kanji(c) => c.to_string(),
        }
    }

    /// The item a `kind` + `item` pair names; `None` for a kind this build
    /// doesn't know, or an item that isn't one of that kind.
    pub fn from_parts(kind: &str, item: &str) -> Option<Self> {
        match CardKind::parse(kind)? {
            CardKind::Word => item.parse().ok().map(Self::Word),
            CardKind::Grammar => item.parse().ok().map(Self::Grammar),
            CardKind::Kanji => {
                let mut chars = item.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Some(Self::Kanji(c)),
                    _ => None,
                }
            }
        }
    }

    /// The item a card id names, `"<kind>:<item>:<direction>"`.
    pub fn from_card_id(id: &str) -> Option<Self> {
        let (kind, rest) = id.split_once(':')?;
        let (item, _direction) = rest.rsplit_once(':')?;
        Self::from_parts(kind, item)
    }

    /// Every direction a card of this item can have, so every card id it
    /// can have. A word has two cards; a kanji and a grammar point one each.
    pub fn directions(self) -> &'static [CardDirection] {
        match self {
            Self::Word(_) => &[CardDirection::Recognition, CardDirection::Recall],
            Self::Kanji(_) => &[CardDirection::Meaning],
            Self::Grammar(_) => &[CardDirection::Recognition],
        }
    }

    /// The card ids this item can have.
    pub fn card_ids(self) -> Vec<String> {
        self.directions().iter().map(|&d| card_id(self, d)).collect()
    }

    pub fn word_seq(self) -> Option<u32> {
        match self {
            Self::Word(n) => Some(n),
            _ => None,
        }
    }

    pub fn kanji(self) -> Option<char> {
        match self {
            Self::Kanji(c) => Some(c),
            _ => None,
        }
    }

    pub fn grammar_seq(self) -> Option<u32> {
        match self {
            Self::Grammar(n) => Some(n),
            _ => None,
        }
    }
}

/// Which side of the item a card asks for. The directions a kind has are
/// [`CardItem::directions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardDirection {
    /// Word: Japanese → meaning. Grammar: pattern → meaning.
    Recognition,
    /// Word: meaning → Japanese.
    Recall,
    /// Kanji: the kanji → its meaning and readings.
    Meaning,
}

impl CardDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recognition => "recognition",
            Self::Recall => "recall",
            Self::Meaning => "meaning",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardStatus {
    Staging,
    Active,
    /// FSRS's computed next-review interval reached the graduation threshold
    /// (`SrsSettings::graduation_interval_days`) — the card is considered
    /// learned. Kept (not deleted) and excluded from Review, shown in Word
    /// List, and reversible via [`crate::idb::reset_card`] or, keeping its
    /// scheduling, [`crate::idb::reactivate_graduated`].
    Graduated,
}

/// A card as persisted in the card stores and sent to the server. Wraps the
/// FSRS scheduling fields with identity (`id`, `item`, `direction`) and
/// lifecycle (`status`) metadata.
///
/// On the wire `item` is two fields, `kind` and `item` (see [`CardItem`]).
/// A card of a kind or direction this build doesn't know fails to parse:
/// the stores keep it as it came and every typed read skips it, so it shows
/// up once the app knows its kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "WireCard", into = "WireCard")]
pub struct SrsCard {
    /// `card_id(item, direction)`; the stores' key.
    pub id: String,
    pub item: CardItem,
    pub direction: CardDirection,
    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: CardState,
    pub last_review_ms: Option<f64>,
    pub added_ms: f64,
    pub status: CardStatus,
    #[serde(default)]
    pub priority: u32,
    /// Every "Again" ever pressed on this card, learning steps included.
    /// Unlike FSRS's `lapses` — which only counts forgetting a card already
    /// in the Review state — this also catches words that never stick in
    /// the first place, which is what the Review tab's trouble list ranks by.
    /// Not scheduling data: FSRS never sees it. Defaulted for cards stored
    /// before the field existed.
    #[serde(default)]
    pub again_count: u32,
    /// Wall-clock ms of the last local write, and the sync merge key. Stamped
    /// by [`crate::idb::put_cards`] so every mutation advances it — including
    /// the ones `last_review_ms` misses (a Staging→Active promotion, a priority
    /// bump) or actively moves backwards ([`SrsCard::reset_progression`], which
    /// clears the review time). Defaulted for cards stored before the field
    /// existed; read those through [`SrsCard::version_ms`].
    #[serde(default)]
    pub updated_ms: f64,
}

/// Whether a logged answer was a scheduled review or a drill, which replays
/// trouble cards without touching their schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewKind {
    Review,
    Drill,
    /// Retracts the answer at `undoes_ms`; the server deletes that row.
    Undo,
}

/// One answer, as kept in the IndexedDB outbox until a sync uploads it. The
/// server keeps the whole log; devices only ever hold what they haven't sent.
/// Identified by `(card_id, reviewed_ms)`, which is also the outbox key, so a
/// re-sent row is ignored server-side. Mirrors `db::ReviewLog` on the server
/// and `ReviewLog` in `extension/src/shared/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewLog {
    pub card_id: String,
    /// The button pressed: 1 = Again … 4 = Easy.
    pub rating: u8,
    pub kind: ReviewKind,
    pub reviewed_ms: f64,
    /// For [`ReviewKind::Undo`]: `reviewed_ms` of the answer being retracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes_ms: Option<f64>,
}

/// A card's id: `"<kind>:<item>:<direction>"`, e.g. `"word:1358280:recall"`,
/// `"kanji:漢:meaning"`, `"grammar:417:recognition"`.
pub fn card_id(item: CardItem, direction: CardDirection) -> String {
    format!("{}:{}:{}", item.kind().as_str(), item.key(), direction.as_str())
}

/// [`SrsCard`] as it is on the wire and in the stores: the item as `kind` +
/// `item` text.
#[derive(Clone, Serialize, Deserialize)]
struct WireCard {
    id: String,
    kind: String,
    item: String,
    direction: CardDirection,
    due_ms: f64,
    stability: f64,
    difficulty: f64,
    reps: u32,
    lapses: u32,
    state: CardState,
    last_review_ms: Option<f64>,
    added_ms: f64,
    status: CardStatus,
    #[serde(default)]
    priority: u32,
    #[serde(default)]
    again_count: u32,
    #[serde(default)]
    updated_ms: f64,
}

/// Whether `card` (a stored or downloaded card, of any kind) has the shape
/// every card has: text `id`, `kind`, `item` and `direction`, none empty, the
/// id being `"<kind>:<item>:<direction>"`. A card without it can't be stored
/// under its key or sent back (the server refuses it), so it is dropped;
/// one with it but of a kind this build doesn't know is kept, unread.
/// Mirrors `isWellFormedCard` in `extension/src/shared/types.ts`.
pub fn is_well_formed_card(card: &serde_json::Value) -> bool {
    let field = |k: &str| card.get(k).and_then(serde_json::Value::as_str).filter(|s| !s.is_empty());
    match (field("id"), field("kind"), field("item"), field("direction")) {
        (Some(id), Some(kind), Some(item), Some(direction)) => {
            id == format!("{kind}:{item}:{direction}")
        }
        _ => false,
    }
}

impl TryFrom<WireCard> for SrsCard {
    type Error = String;

    fn try_from(w: WireCard) -> Result<Self, String> {
        let item = CardItem::from_parts(&w.kind, &w.item)
            .ok_or_else(|| format!("card {}: unknown item {}:{}", w.id, w.kind, w.item))?;
        if !item.directions().contains(&w.direction) {
            return Err(format!("card {}: a {} card has no {:?} side", w.id, w.kind, w.direction));
        }
        // The id is the store key; one that disagrees with the fields would
        // file the card where `get_card` and `delete_card` never look.
        let id = card_id(item, w.direction);
        if w.id != id {
            return Err(format!("card {}: its key is {id}", w.id));
        }
        Ok(Self {
            id: w.id,
            item,
            direction: w.direction,
            due_ms: w.due_ms,
            stability: w.stability,
            difficulty: w.difficulty,
            reps: w.reps,
            lapses: w.lapses,
            state: w.state,
            last_review_ms: w.last_review_ms,
            added_ms: w.added_ms,
            status: w.status,
            priority: w.priority,
            again_count: w.again_count,
            updated_ms: w.updated_ms,
        })
    }
}

impl From<SrsCard> for WireCard {
    fn from(c: SrsCard) -> Self {
        Self {
            id: c.id,
            kind: c.item.kind().as_str().to_owned(),
            item: c.item.key(),
            direction: c.direction,
            due_ms: c.due_ms,
            stability: c.stability,
            difficulty: c.difficulty,
            reps: c.reps,
            lapses: c.lapses,
            state: c.state,
            last_review_ms: c.last_review_ms,
            added_ms: c.added_ms,
            status: c.status,
            priority: c.priority,
            again_count: c.again_count,
            updated_ms: c.updated_ms,
        }
    }
}

/// The schema-2 card: keyed by a `u32` `sequence` whose range told the kind
/// apart (an `ent_seq`, a kanji's code point, a grammar hash with the top bit
/// set), id `"<sequence>::<direction>"`. Read from exports and backups made
/// before the card key change; see [`SrsCardV2::upgrade`].
#[derive(Debug, Clone, Deserialize)]
pub struct SrsCardV2 {
    pub sequence: u32,
    pub direction: String,
    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: CardState,
    #[serde(default)]
    pub last_review_ms: Option<f64>,
    pub added_ms: f64,
    pub status: CardStatus,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub again_count: u32,
    #[serde(default)]
    pub updated_ms: f64,
}

/// The current key of a schema-2 card, from its `sequence` and `direction`.
/// `None` for a grammar card: its number was a hash of the entry's slug,
/// which nothing maps back (`docs/phase-4-grammar-cards.md`, "Card keys").
pub fn v2_key(sequence: u32, direction: &str) -> Option<(CardItem, CardDirection)> {
    match direction {
        "recognition" => Some((CardItem::Word(sequence), CardDirection::Recognition)),
        "recall" => Some((CardItem::Word(sequence), CardDirection::Recall)),
        "kanji" => char::from_u32(sequence).map(|c| (CardItem::Kanji(c), CardDirection::Meaning)),
        _ => None,
    }
}

/// [`v2_key`] on a whole schema-2 card id, `"<sequence>::<direction>"`: the
/// current id, or `None` when it has none.
pub fn v2_card_id(id: &str) -> Option<String> {
    let (sequence, direction) = id.split_once("::")?;
    let (item, direction) = v2_key(sequence.parse().ok()?, direction)?;
    Some(card_id(item, direction))
}

impl SrsCardV2 {
    /// The same card under the current key, every field kept; `None` for a
    /// grammar card ([`v2_key`]).
    pub fn upgrade(self) -> Option<SrsCard> {
        let (item, direction) = v2_key(self.sequence, &self.direction)?;
        Some(SrsCard {
            id: card_id(item, direction),
            item,
            direction,
            due_ms: self.due_ms,
            stability: self.stability,
            difficulty: self.difficulty,
            reps: self.reps,
            lapses: self.lapses,
            state: self.state,
            last_review_ms: self.last_review_ms,
            added_ms: self.added_ms,
            status: self.status,
            priority: self.priority,
            again_count: self.again_count,
            updated_ms: self.updated_ms,
        })
    }
}

/// Card shape used by exports that predate the `version` field. These cards were
/// keyed by the headword string (`word`) — the `id` was `{word}::{direction}` —
/// rather than the JMdict `sequence`. Word cards only. Imported by resolving `word` to its
/// sequence via the dictionary, then [`upgrade`](SrsCardV1::upgrade)-ing.
#[derive(Debug, Clone, Deserialize)]
pub struct SrsCardV1 {
    pub word: String,
    pub direction: CardDirection,
    pub due_ms: f64,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: CardState,
    #[serde(default)]
    pub last_review_ms: Option<f64>,
    pub added_ms: f64,
    pub status: CardStatus,
}

impl SrsCardV1 {
    /// Upgrade to the current [`SrsCard`], re-keying by the resolved JMdict
    /// `sequence` (the legacy `word`-based `id` is discarded).
    pub fn upgrade(self, sequence: u32) -> SrsCard {
        let item = CardItem::Word(sequence);
        SrsCard {
            id: card_id(item, self.direction),
            item,
            direction: self.direction,
            due_ms: self.due_ms,
            stability: self.stability,
            difficulty: self.difficulty,
            reps: self.reps,
            lapses: self.lapses,
            state: self.state,
            last_review_ms: self.last_review_ms,
            added_ms: self.added_ms,
            status: self.status,
            priority: 0,
            again_count: 0,
            updated_ms: self.last_review_ms.unwrap_or(0.0).max(self.added_ms),
        }
    }
}

impl SrsCard {
    /// Build a new staging card from FSRS-fresh scheduling fields.
    pub fn new(item: CardItem, direction: CardDirection, now_ms: f64) -> Self {
        debug_assert!(item.directions().contains(&direction), "{item:?} has no {direction:?} card");
        Self {
            id: card_id(item, direction),
            item,
            direction,
            due_ms: now_ms,
            stability: 0.0,
            difficulty: 0.0,
            reps: 0,
            lapses: 0,
            state: CardState::New,
            last_review_ms: None,
            added_ms: now_ms,
            status: CardStatus::Staging,
            priority: 0,
            again_count: 0,
            updated_ms: now_ms,
        }
    }

    /// The new staging cards of `item`, one per direction it has.
    pub fn new_cards(item: CardItem, now_ms: f64) -> Vec<Self> {
        item.directions()
            .iter()
            .map(|&d| Self::new(item, d, now_ms))
            .collect()
    }

    /// A new kanji card for `kanji`, straight into the active deck: kanji cards
    /// are derived from words already being reviewed, so there is no staging.
    pub fn new_kanji(kanji: char, now_ms: f64) -> Self {
        Self {
            status: CardStatus::Active,
            ..Self::new(CardItem::Kanji(kanji), CardDirection::Meaning, now_ms)
        }
    }

    /// A new grammar card for the entry numbered `seq`, into staging like
    /// words: grammar points are added by hand and enter at the user's pace.
    pub fn new_grammar(seq: u32, now_ms: f64) -> Self {
        Self::new(CardItem::Grammar(seq), CardDirection::Recognition, now_ms)
    }

    pub fn kind(&self) -> CardKind {
        self.item.kind()
    }

    /// Whether this card reviews a JMdict word.
    pub fn is_word(&self) -> bool {
        self.kind() == CardKind::Word
    }

    /// Whether this card reviews a single kanji.
    pub fn is_kanji(&self) -> bool {
        self.kind() == CardKind::Kanji
    }

    /// Whether this card reviews a grammar point.
    pub fn is_grammar(&self) -> bool {
        self.kind() == CardKind::Grammar
    }

    /// The JMdict `ent_seq` a word card reviews; `None` for other cards.
    pub fn word_seq(&self) -> Option<u32> {
        self.item.word_seq()
    }

    /// The kanji a kanji card reviews; `None` for other cards.
    pub fn kanji_char(&self) -> Option<char> {
        self.item.kanji()
    }

    /// The grammar entry's `seq` a grammar card reviews; `None` for others.
    pub fn grammar_seq(&self) -> Option<u32> {
        self.item.grammar_seq()
    }

    /// The card's merge version. Falls back to the review/added times for cards
    /// written before `updated_ms` existed, so the first sync after upgrading
    /// compares something meaningful instead of an all-zero tie. Both inputs are
    /// already agreed on by every device, so the fallback is deterministic.
    pub fn version_ms(&self) -> f64 {
        if self.updated_ms > 0.0 {
            self.updated_ms
        } else {
            self.last_review_ms.unwrap_or(0.0).max(self.added_ms)
        }
    }

    pub fn to_scheduling(&self) -> srs_core::SrsCard {
        srs_core::SrsCard {
            // FSRS doesn't read it; srs-core only carries it through.
            sequence: 0,
            added_ms: self.added_ms,
            due_ms: self.due_ms,
            stability: self.stability,
            difficulty: self.difficulty,
            reps: self.reps,
            lapses: self.lapses,
            state: self.state,
            last_review_ms: self.last_review_ms,
        }
    }

    pub fn apply_scheduling(&mut self, s: &srs_core::SrsCard) {
        self.due_ms = s.due_ms;
        self.stability = s.stability;
        self.difficulty = s.difficulty;
        self.reps = s.reps;
        self.lapses = s.lapses;
        self.state = s.state;
        self.last_review_ms = s.last_review_ms;
    }

    /// Resets FSRS progress back to a fresh "New" card while preserving
    /// identity/status/`added_ms`/`priority`, and clears the `again_count`
    /// failure history along with it. Destructive; callers must
    /// confirm with the user first.
    pub fn reset_progression(&mut self, now_ms: f64) {
        self.due_ms = now_ms;
        self.stability = 0.0;
        self.difficulty = 0.0;
        self.reps = 0;
        self.lapses = 0;
        self.again_count = 0;
        self.state = CardState::New;
        self.last_review_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_whose_id_disagrees_with_its_key_is_refused() {
        let mut v = serde_json::to_value(SrsCard::new_kanji('猫', 0.0)).unwrap();
        assert!(serde_json::from_value::<SrsCard>(v.clone()).is_ok());
        v["id"] = "kanji:犬:meaning".into();
        assert!(serde_json::from_value::<SrsCard>(v.clone()).is_err());
        assert!(!is_well_formed_card(&v));
    }

    #[test]
    fn card_ids_name_kind_item_and_direction() {
        assert_eq!(card_id(CardItem::Word(1_358_280), CardDirection::Recall), "word:1358280:recall");
        assert_eq!(card_id(CardItem::Kanji('漢'), CardDirection::Meaning), "kanji:漢:meaning");
        assert_eq!(card_id(CardItem::Grammar(417), CardDirection::Recognition), "grammar:417:recognition");
        for item in [CardItem::Word(1_358_280), CardItem::Kanji('漢'), CardItem::Grammar(417)] {
            for id in item.card_ids() {
                assert_eq!(CardItem::from_card_id(&id), Some(item), "{id}");
            }
        }
        assert_eq!(CardItem::Word(1).card_ids(), ["word:1:recognition", "word:1:recall"]);
        assert_eq!(CardItem::from_card_id("1358280::recall"), None);
        assert_eq!(CardItem::from_card_id("cloze:3:front"), None);
        assert_eq!(CardItem::from_parts("kanji", "漢字"), None);
        assert_eq!(CardItem::from_parts("word", "x"), None);
    }

    #[test]
    fn a_card_is_kind_and_item_on_the_wire() {
        let c = SrsCard::new_kanji('猫', 5.0);
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["id"], "kanji:猫:meaning");
        assert_eq!(json["kind"], "kanji");
        assert_eq!(json["item"], "猫");
        assert_eq!(json["direction"], "meaning");
        assert!(json.get("sequence").is_none());
        let back: SrsCard = serde_json::from_value(json).unwrap();
        assert_eq!(back.item, CardItem::Kanji('猫'));
        assert_eq!(back.id, c.id);
    }

    #[test]
    fn a_card_this_build_does_not_know_does_not_parse() {
        let mut json = serde_json::to_value(SrsCard::new_grammar(7, 0.0)).unwrap();
        json["direction"] = "production".into();
        assert!(serde_json::from_value::<SrsCard>(json.clone()).is_err());
        json["direction"] = "recognition".into();
        json["kind"] = "cloze".into();
        assert!(serde_json::from_value::<SrsCard>(json.clone()).is_err());
        // A known kind with a side it doesn't have.
        json["kind"] = "kanji".into();
        json["item"] = "猫".into();
        assert!(serde_json::from_value::<SrsCard>(json).is_err());
    }

    #[test]
    fn a_grammar_card_is_keyed_by_its_entry_and_is_no_word() {
        let c = SrsCard::new_grammar(7, 5.0);
        assert_eq!(c.item, CardItem::Grammar(7));
        assert_eq!(c.id, "grammar:7:recognition");
        assert_eq!(c.status, CardStatus::Staging);
        assert_eq!(c.kind(), CardKind::Grammar);
        assert!(c.is_grammar() && !c.is_word() && !c.is_kanji());
        assert_eq!((c.kanji_char(), c.word_seq(), c.grammar_seq()), (None, None, Some(7)));
    }

    #[test]
    fn word_and_kanji_cards_keep_their_kind() {
        let words = SrsCard::new_cards(CardItem::Word(1_000_000), 0.0);
        assert_eq!(words.len(), 2);
        assert!(words.iter().all(SrsCard::is_word));
        assert_eq!(SrsCard::new_kanji('猫', 0.0).kind(), CardKind::Kanji);
    }

    #[test]
    fn schema_2_keys_map_to_the_current_ones() {
        assert_eq!(v2_card_id("1358280::recognition").as_deref(), Some("word:1358280:recognition"));
        assert_eq!(v2_card_id("1358280::recall").as_deref(), Some("word:1358280:recall"));
        assert_eq!(v2_card_id("29483::kanji").as_deref(), Some("kanji:猫:meaning"));
        // Grammar numbers were hashes of the slug: nothing maps them back.
        assert_eq!(v2_card_id("3444285014::grammar"), None);
        assert_eq!(v2_card_id("word:1:recall"), None);
        assert_eq!(v2_card_id("x::recall"), None);
    }
}
