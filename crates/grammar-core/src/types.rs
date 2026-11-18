//! The grammar entry model: one point of `data/grammar.toml`, Yomeru's own
//! grammar dictionary, as the UIs render it.

use serde::{Deserialize, Serialize};

/// One grammar point.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrammarEntry {
    /// ASCII slug ("te-mo-ii"): the readable key, for links and lookups.
    pub id: String,
    /// The entry's number, issued once from `grammar.toml`'s `next_seq` and
    /// never reused: review cards key on it.
    pub seq: u32,
    /// The headword as learners look it up, e.g. "てもいい".
    pub pattern: String,
    /// Other spellings and variants a lookup also finds.
    pub forms: Vec<String>,
    /// "N5"…"N1" (Bunpro's level for the point).
    pub jlpt: String,
    /// One line, for the popup and the card's back.
    pub meaning: String,
    /// How it attaches, one line per construction, e.g. "Verb［て］ + もいい".
    pub formation: Vec<String>,
    /// What it does and when to use it, a short paragraph.
    pub explanation: String,
    /// Nuances, pitfalls, look-alikes; may be empty.
    pub notes: String,
    /// Patterns worth comparing, with the headword to show and search.
    pub related: Vec<Related>,
    /// Ids of the entries sharing a spelling with this one; a lookup shows
    /// them together.
    pub homographs: Vec<String>,
    pub examples: Vec<Example>,
    /// False keeps the entry out of the hover popup: search only.
    pub hover: bool,
    /// How a production answer is checked for this point.
    pub check: AnswerCheck,
}

/// How a production review checks that the typed answer uses the point.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnswerCheck {
    /// The pattern or one of its `forms` appears in the answer, conjugated
    /// or not: てから, ちゃだめ.
    #[default]
    Forms,
    /// A verb of the answer is in one of these forms: 食べさせた for the
    /// causative. For the points that are a conjugation, not a fixed string.
    Inflection { forms: Vec<Inflection> },
    /// No automatic check: the user compares with the reference and rates.
    /// For what neither finds reliably (the く adverb, the て join).
    SelfGraded,
}

/// A verb form an [`AnswerCheck::Inflection`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inflection {
    Passive,
    Potential,
    Causative,
    CausativePassive,
    Volitional,
    Imperative,
}

impl Inflection {
    pub const ALL: [Inflection; 6] = [
        Self::Passive,
        Self::Potential,
        Self::Causative,
        Self::CausativePassive,
        Self::Volitional,
        Self::Imperative,
    ];

    /// The name `grammar.toml` uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Potential => "potential",
            Self::Causative => "causative",
            Self::CausativePassive => "causative-passive",
            Self::Volitional => "volitional",
            Self::Imperative => "imperative",
        }
    }
}

/// A related point: its id, and its pattern for the link's label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub id: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    /// The sentence, the pattern highlighted.
    pub ja: Rich,
    pub en: String,
    /// Tatoeba sentence ids, `<ja>_<en>`; empty for our own sentences.
    pub tatoeba: String,
    /// The example's key within its entry, which reports and verdicts point
    /// at: the `tatoeba` pair, or `y<n>` for our own sentences.
    pub key: String,
}

/// Text with the pattern marked: a run of spans.
pub type Rich = Vec<Span>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Span {
    Text {
        text: String,
    },
    /// The grammar point itself within an example.
    Highlight {
        text: String,
    },
}

impl Span {
    pub fn text(&self) -> &str {
        match self {
            Span::Text { text } | Span::Highlight { text } => text,
        }
    }
}

/// The plain text of a rich run.
pub fn plain(rich: &[Span]) -> String {
    rich.iter().map(Span::text).collect()
}

/// Splits `ja` at its `{…}` mark: "この本を借り{てもいい}ですか。".
/// `None` unless there is exactly one well-formed mark.
pub fn parse_marked(ja: &str) -> Option<Rich> {
    let (before, rest) = ja.split_once('{')?;
    let (mark, after) = rest.split_once('}')?;
    if mark.is_empty() || [before, mark, after].iter().any(|s| s.contains(['{', '}'])) {
        return None;
    }
    let text = |s: &str| Span::Text { text: s.to_owned() };
    let mut out = Vec::with_capacity(3);
    if !before.is_empty() {
        out.push(text(before));
    }
    out.push(Span::Highlight {
        text: mark.to_owned(),
    });
    if !after.is_empty() {
        out.push(text(after));
    }
    Some(out)
}

impl GrammarEntry {
    /// Every spelling the entry is found by: the pattern, then its forms.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.pattern.as_str()).chain(self.forms.iter().map(String::as_str))
    }
}
