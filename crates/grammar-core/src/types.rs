//! The grammar entry model: what the converter keeps of a Yomitan grammar
//! card, in a shape the UIs can render without knowing Yomitan's structured
//! content.

use serde::{Deserialize, Serialize};

/// Facts about the imported dictionary, from its `index.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrammarMeta {
    pub title: String,
    /// The dictionary's own revision, e.g. "2026.11.08". Compared with the
    /// published `index.json` to offer an update.
    pub revision: String,
    pub attribution: String,
    pub url: String,
    /// Where the publisher's current `index.json` lives, if it is updatable.
    pub index_url: String,
    pub download_url: String,
}

/// One grammar point.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrammarEntry {
    /// Headword as the dictionary spells it, e.g. "ようにする".
    pub term: String,
    /// Kana reading of a kanji headword; empty when the term is its own.
    pub reading: String,
    /// Yomitan deinflection rule names ("v1", "v5", "vs", "adj-i"…), space
    /// separated; empty for patterns that don't conjugate. Kept for a later
    /// check of deinflected matches.
    pub rules: String,
    /// One-line meaning shown before any source is opened.
    pub meaning: String,
    /// How the pattern attaches, e.g. "動詞辞書形+ように+する".
    pub structure: String,
    /// "N1"…"N5", or empty.
    pub jlpt: String,
    /// What each source says, in the dictionary's order.
    pub sources: Vec<SourceBlock>,
    /// Headwords this entry only points to ("see AながらB").
    pub see_also: Vec<String>,
    /// The point's page on the publisher's website, with every explanation
    /// and the illustrations this format leaves out.
    pub website: String,
}

/// One source's contribution to a grammar point.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBlock {
    /// e.g. "DoJG 日本語文法辞典(全集)".
    pub name: String,
    /// The level this source gives the point, e.g. "N4"; may be empty.
    pub level: String,
    pub senses: Vec<Sense>,
}

/// One meaning of the point within a source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sense {
    /// Optional heading of the sense, e.g. "4 Morae Adverbs".
    pub label: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Block {
    /// A paragraph of explanation.
    Paragraph {
        kind: ParagraphKind,
        text: Rich,
    },
    /// Formation patterns, e.g. "Verb［る］ + ように + する".
    Patterns {
        items: Vec<Rich>,
    },
    Examples {
        items: Vec<Example>,
    },
}

/// What a paragraph is, so the UI can style it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParagraphKind {
    Plain,
    /// English rendering of the source's prose, written by the source.
    Translation,
    Heading,
    /// An example given within the explanation.
    InlineExample,
    /// A rewrite of the inline example before it ("→ …", "＝ …").
    Derivation,
    /// A note on the inline example before it ("【「持つ」は動詞】").
    Annotation,
    /// The label introducing inline examples ("［例］").
    ExampleLabel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    pub ja: Rich,
    /// The source's translation; empty when it gives none.
    pub en: String,
}

/// Text with inline styling: a run of spans.
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
    /// A label inside a paragraph, e.g. "【熱・暑】".
    Label {
        text: String,
    },
    Ruby {
        text: String,
        ruby: String,
    },
    /// A reference to another headword.
    Link {
        text: String,
        query: String,
    },
    LineBreak,
}

impl Span {
    pub fn text(&self) -> &str {
        match self {
            Span::Text { text }
            | Span::Highlight { text }
            | Span::Label { text }
            | Span::Ruby { text, .. }
            | Span::Link { text, .. } => text,
            Span::LineBreak => "\n",
        }
    }
}

/// The plain text of a rich run, ruby dropped.
pub fn plain(rich: &[Span]) -> String {
    rich.iter().map(Span::text).collect()
}

// ── Display helpers ─────────────────────────────────────────────────────

/// `s` without whitespace, for comparing texts laid out differently.
fn squeezed(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

impl GrammarEntry {
    /// The one-line meaning, whole. Some arrive cut short ("…gratitude
    /// after…"); the source paragraph they were cut from replaces them.
    pub fn full_meaning(&self) -> String {
        let stem = self
            .meaning
            .strip_suffix('…')
            .or_else(|| self.meaning.strip_suffix("..."))
            .map(str::trim_end);
        let Some(stem) = stem.filter(|s| !s.is_empty()) else {
            return self.meaning.clone();
        };
        self.sources
            .iter()
            .flat_map(|s| &s.senses)
            .flat_map(|s| &s.blocks)
            .find_map(|b| match b {
                Block::Paragraph { text, .. } => {
                    let t = plain(text);
                    t.starts_with(stem).then_some(t)
                }
                _ => None,
            })
            .unwrap_or_else(|| self.meaning.clone())
    }

    /// The source to show first: the one explaining the most in English
    /// (Latin letters of its paragraphs), the first on a tie or when none
    /// has English.
    pub fn main_source(&self) -> usize {
        let english = |s: &SourceBlock| -> usize {
            s.senses
                .iter()
                .flat_map(|s| s.prose())
                .map(|(_, text)| {
                    plain(text)
                        .chars()
                        .filter(char::is_ascii_alphabetic)
                        .count()
                })
                .sum()
        };
        self.sources
            .iter()
            .enumerate()
            .max_by_key(|&(i, s)| (english(s), std::cmp::Reverse(i)))
            .map_or(0, |(i, _)| i)
    }
}

impl Sense {
    pub fn examples(&self) -> impl Iterator<Item = &Example> {
        self.blocks.iter().flat_map(|b| match b {
            Block::Examples { items } => items.as_slice(),
            _ => &[],
        })
    }

    pub fn patterns(&self) -> impl Iterator<Item = &Rich> {
        self.blocks.iter().flat_map(|b| match b {
            Block::Patterns { items } => items.as_slice(),
            _ => &[],
        })
    }

    /// The explanation's paragraphs, without those repeating one of the
    /// sense's examples or its translation: some sources write their
    /// examples into the explanation as well as listing them.
    pub fn prose(&self) -> impl Iterator<Item = (ParagraphKind, &Rich)> {
        let repeated: Vec<String> = self
            .examples()
            .flat_map(|e| [squeezed(&plain(&e.ja)), squeezed(&e.en)])
            .filter(|s| !s.is_empty())
            .collect();
        self.blocks.iter().filter_map(move |b| match b {
            Block::Paragraph { kind, text } if !repeated.contains(&squeezed(&plain(text))) => {
                Some((*kind, text))
            }
            _ => None,
        })
    }
}
