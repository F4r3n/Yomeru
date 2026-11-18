//! `data/grammar.toml` → `grammar.bin`, at build time (`cargo xtask
//! build-grammar`). The TOML's header comment documents its fields; the
//! grammar-points skill's `check.py` checks the content, this checks what
//! the lookups rely on.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::format;
use crate::types::{Example, GrammarEntry, Related, parse_marked};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    /// The `seq` the next new entry takes.
    next_seq: u32,
    point: Vec<Point>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    id: String,
    seq: u32,
    pattern: String,
    #[serde(default)]
    forms: Vec<String>,
    jlpt: String,
    meaning: String,
    formation: Vec<String>,
    explanation: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    related: Vec<String>,
    #[serde(default)]
    homographs: Vec<String>,
    examples: Vec<RawExample>,
    #[serde(default = "yes")]
    hover: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExample {
    ja: String,
    en: String,
    tatoeba: Option<String>,
    source: Option<String>,
    /// `y<n>`, on our own sentences only: their key within the entry.
    id: Option<String>,
}

/// What [`compile`] produced.
pub struct Compiled {
    /// The `grammar.bin` bytes.
    pub bytes: Vec<u8>,
    pub entries: usize,
    /// Related ids that aren't entries yet: left out of the links.
    pub warnings: Vec<String>,
}

/// Parses `data/grammar.toml` and writes `grammar.bin`. Fails on anything a
/// lookup would get wrong: duplicate ids or `seq` numbers, a bad `{…}` mark,
/// a spelling two entries share without naming each other homographs.
pub fn compile(toml_text: &str) -> anyhow::Result<Compiled> {
    let file: File = toml::from_str(toml_text).context("parsing grammar.toml")?;
    let mut warnings = Vec::new();

    let mut ids: HashSet<&str> = HashSet::new();
    let mut seqs: HashMap<u32, &str> = HashMap::new();
    for p in &file.point {
        let slug = !p.id.is_empty()
            && p.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !slug {
            bail!("{}: id must be a lowercase ASCII slug", p.id);
        }
        if !ids.insert(&p.id) {
            bail!("{}: duplicate id", p.id);
        }
        // Review cards key on this number. `next_seq` only goes up, so a
        // number below it that no entry has belonged to a removed one and
        // stays unused.
        if p.seq == 0 || p.seq >= file.next_seq {
            bail!(
                "{}: seq {} must be from 1 to next_seq - 1 ({}): a new entry takes next_seq and raises it",
                p.id,
                p.seq,
                file.next_seq - 1
            );
        }
        if let Some(other) = seqs.insert(p.seq, &p.id) {
            bail!("{}: seq {} is already {other}'s", p.id, p.seq);
        }
        if !matches!(p.jlpt.as_str(), "N5" | "N4" | "N3" | "N2" | "N1") {
            bail!("{}: jlpt must be one of N5…N1, not {:?}", p.id, p.jlpt);
        }
        if p.pattern.trim().is_empty() || p.meaning.trim().is_empty() {
            bail!("{}: empty pattern or meaning", p.id);
        }
    }

    // A spelling belongs to one entry, or to entries that are each other's
    // homographs.
    let mut owners: HashMap<&str, Vec<&Point>> = HashMap::new();
    for p in &file.point {
        let keys: HashSet<&str> = std::iter::once(p.pattern.as_str())
            .chain(p.forms.iter().map(String::as_str))
            .collect();
        for key in keys {
            owners.entry(key).or_default().push(p);
        }
        for h in &p.homographs {
            let Some(other) = file.point.iter().find(|o| &o.id == h) else {
                bail!("{}: homograph {h} is not an entry", p.id);
            };
            if !other.homographs.contains(&p.id) {
                bail!("{}: homograph {h} doesn't name it back", p.id);
            }
        }
    }
    for (key, ps) in &owners {
        for (i, a) in ps.iter().enumerate() {
            for b in ps.iter().skip(i + 1) {
                if !a.homographs.contains(&b.id) {
                    bail!(
                        "{} and {} both spell {key}: name each other in homographs",
                        a.id,
                        b.id
                    );
                }
            }
        }
    }

    let pattern_of: HashMap<String, String> = file
        .point
        .iter()
        .map(|p| (p.id.clone(), p.pattern.clone()))
        .collect();
    let mut entries = Vec::with_capacity(file.point.len());
    for p in file.point {
        let mut examples = Vec::with_capacity(p.examples.len());
        let mut keys: HashSet<String> = HashSet::new();
        for ex in p.examples {
            let ja = parse_marked(&ex.ja).with_context(|| {
                format!("{}: mark the pattern once with {{…}}: {}", p.id, ex.ja)
            })?;
            // Reports and verdicts point at an example by its key, so a key
            // names one example of the entry. Our own sentences have no
            // corpus id: they carry `y<n>`, a number never given again
            // within the entry.
            let (tatoeba, key) = match (ex.tatoeba, ex.source.as_deref(), ex.id) {
                (Some(pair), None, None) => (pair.clone(), pair),
                (None, Some("yomeru"), Some(id)) => {
                    let numbered = id.strip_prefix('y').is_some_and(|n| {
                        !n.is_empty()
                            && !n.starts_with('0')
                            && n.bytes().all(|b| b.is_ascii_digit())
                    });
                    if !numbered {
                        bail!("{}: example id {id:?} must be y1, y2…: {}", p.id, ex.ja);
                    }
                    (String::new(), id)
                }
                (None, Some("yomeru"), None) => {
                    bail!("{}: our own example needs id = \"y<n>\": {}", p.id, ex.ja)
                }
                (_, _, Some(_)) => bail!(
                    "{}: only our own examples take an id, a Tatoeba one is keyed by its pair: {}",
                    p.id,
                    ex.ja
                ),
                _ => bail!(
                    "{}: an example has either `tatoeba` or source = \"yomeru\": {}",
                    p.id,
                    ex.ja
                ),
            };
            if !keys.insert(key.clone()) {
                bail!("{}: two examples are keyed {key}", p.id);
            }
            examples.push(Example {
                ja,
                en: ex.en,
                tatoeba,
                key,
            });
        }
        let mut related = Vec::with_capacity(p.related.len());
        for id in p.related {
            match pattern_of.get(&id) {
                Some(pattern) => related.push(Related {
                    pattern: pattern.clone(),
                    id,
                }),
                None => warnings.push(format!("{}: related {id} isn't written yet", p.id)),
            }
        }
        entries.push(GrammarEntry {
            id: p.id,
            seq: p.seq,
            pattern: p.pattern,
            forms: p.forms,
            jlpt: p.jlpt,
            meaning: p.meaning,
            formation: p.formation,
            explanation: p.explanation.trim().to_owned(),
            notes: p.notes.trim().to_owned(),
            related,
            homographs: p.homographs,
            examples,
            hover: p.hover,
        });
    }

    let bytes = format::write(&entries)?;
    Ok(Compiled {
        bytes,
        entries: entries.len(),
        warnings,
    })
}
