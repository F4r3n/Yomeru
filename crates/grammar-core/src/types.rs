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
    /// Deinflection reasons ([`deinflect::Reason::name`]) a word carries
    /// when it shows this point: `adverbial` for the く adverb (速く). For
    /// the sentence breakdown only; the answer check reads `check`.
    #[serde(default)]
    pub reads: Vec<String>,
    /// What the point attaches to ([`AFTER_KINDS`]): the sentence
    /// breakdown finds it only after one of them (に行く after a ます-stem
    /// or a する noun, not 学校に行く). Empty: after anything.
    #[serde(default)]
    pub after: Vec<String>,
    /// The point ends a sentence (the の of 行くの？): the breakdown finds
    /// it only before the end, punctuation or a sentence-ending particle
    /// (よ, ね), not before the は of 行くのは.
    #[serde(default)]
    pub ends: bool,
    /// The point never ends a sentence (the にしろ of 雨にしろ、…): the
    /// breakdown skips it where [`Self::ends`] points are found, and
    /// にしろ at the end reads as にする's command.
    #[serde(default)]
    pub continues: bool,
    /// The point's final に or と is never before する or なる (以上に,
    /// "more than"; ないと, "must"): there it is にする's, になる's or
    /// とする's, and 以上にしろ reads as 以上 + にする, いないとして as a
    /// quote + とする.
    #[serde(default)]
    pub no_suru: bool,
    /// The point's final と is never before a verb of saying or thinking
    /// (言う, 思う): there it quotes (いいと言う), and the conditional と
    /// isn't found.
    #[serde(default)]
    pub no_quote: bool,
    /// The point ends a clause and is never before a particle (の結果、…):
    /// before が, を, は… it is the noun of its spelling (試験の結果が出た).
    #[serde(default)]
    pub no_particle: bool,
    /// The point also ends a word the dictionary lists whole (新型, 痛み,
    /// 三回目): the breakdown finds it inside a word, ending it, when its
    /// [`Self::after`] (which it needs) fits the rest of the word.
    #[serde(default)]
    pub suffix: bool,
    /// The point also opens a word the dictionary lists whole (各国,
    /// 真っ暗): the breakdown finds it at a word's start, ending inside
    /// it, when the rest is a word or an adjective's stem.
    #[serde(default)]
    pub prefix: bool,
    /// Inside a word the dictionary lists whole (食べかける, 出かける),
    /// the breakdown finds the point only when a gloss of the word's
    /// first sense holds one of these (`start *ing`: `*` stands for a word):
    /// 食べかける "to start eating" is かける "half-done", 出かける "to go
    /// out" isn't. Empty: anywhere.
    #[serde(default)]
    pub inside_glosses: Vec<String>,
}

/// The kinds of word `after` can name: `verb-plain` (a verb in its
/// dictionary form: 走る, 電話する), `masu-stem` (食べ, 遊び), `suru-noun`
/// (散歩, 勉強), `amount` (a number, with or without its counter: 三十分,
/// 100, 五つ), `noun-or-demonstrative` (東京, この), `noun-or-particle`
/// (私, 由利さん, から), `predicate` (a verb, adjective or ending,
/// conjugated or not: 行くの, 高いの, 行ったのか; a form starting with な
/// also after a noun or な-adjective: 休みなの), `adjective-stem` (寒,
/// 欲し, 嫌, 行きた: what がる follows), `te-form` (a verb's て-form: 見て
/// of 見て回る).
pub const AFTER_KINDS: [&str; 9] = [
    "verb-plain",
    "masu-stem",
    "suru-noun",
    "amount",
    "noun-or-demonstrative",
    "noun-or-particle",
    "predicate",
    "adjective-stem",
    "te-form",
];

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

    /// The deinflection reasons a verb in this form carries, all of them.
    pub fn reasons(self) -> deinflect::Reasons {
        use deinflect::{Reason, Reasons};
        match self {
            Self::Passive => Reasons::of(&[Reason::Passive]),
            Self::Potential => Reasons::of(&[Reason::Potential]),
            Self::Causative => Reasons::of(&[Reason::Causative]),
            Self::CausativePassive => Reasons::of(&[Reason::Causative, Reason::Passive]),
            Self::Volitional => Reasons::of(&[Reason::Volitional]),
            Self::Imperative => Reasons::of(&[Reason::Imperative]),
        }
    }

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
    /// Other correct answers to the English, plain text: production
    /// reviews compare an answer with the closest of `ja` and these.
    #[serde(default)]
    pub also: Vec<String>,
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
    /// [`Self::reads`] as reasons, all together.
    pub fn read_reasons(&self) -> deinflect::Reasons {
        self.reads
            .iter()
            .filter_map(|n| deinflect::Reason::from_name(n))
            .fold(deinflect::Reasons::EMPTY, |acc, r| {
                acc.union(deinflect::Reasons::of(&[r]))
            })
    }

    /// Every spelling the entry is found by: the pattern, then its forms.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.pattern.as_str()).chain(self.forms.iter().map(String::as_str))
    }
}
