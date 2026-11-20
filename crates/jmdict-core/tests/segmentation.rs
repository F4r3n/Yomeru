//! How well `segment::segment` splits real sentences, measured against
//! the word split of the Tanaka corpus (`examples.utf`'s `B:` lines). A
//! report, not a pass/fail test: it prints the scores and the commonest
//! disagreements, so a change to the segmenter can be compared with the
//! last run.
//!
//! ```sh
//! JMDICT_BIN=path/to/jmdict.bin cargo test --release -p jmdict-core \
//!     --test segmentation -- --ignored --nocapture
//! ```
//!
//! `JMDICT_BIN` must be a `jmdict-types/full` build (see `real_dict.rs`).
//! `SEG_REPORT=path.tsv` also writes every disagreement, one per line, and
//! `SEG_COSTS=no_freq=1500,word=900` tries other costs (`segment::Costs`).
//!
//! Scores when the lattice came in (2026-11-19), word / boundary F1, right
//! entry, sentences split exactly: longest match at each point 79.3 / 89.2
//! / 94.8 / 38.9%; lattice 88.5 / 94.8 / 97.7 / 56.5%; with the ichidan check, `vs`
//! nouns kept out of verbs and the lattice-only verb forms (2026-11-19)
//! 88.9 / 95.0 / 97.7 / 57.6%.
//!
//! The corpus' split is a convention as much as a truth: it sometimes keeps
//! a particle with its word (私{私の}) or a whole ending as one token
//! (ですか). A disagreement is a case to look at, not always an error.

#![allow(
    clippy::panic,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use jmdict_core::segment::Word;
use jmdict_types::ArchivedWordEntry;

/// One word of a `B:` line, located in its sentence.
struct Gold {
    start: usize,
    len: usize,
    /// The dictionary form the corpus gives (無い for ない).
    headword: String,
    /// An explicit JMdict sequence, `で(#2028980)`.
    sequence: Option<u32>,
}

/// One `B:` token: `headword(reading)[sense]{surface}~`, or
/// `headword(#seq)`. Returns the headword, the surface as written in the
/// sentence and the sequence if any.
fn parse_token(tok: &str) -> (String, String, Option<u32>) {
    let tok = tok.trim_end_matches('~');
    let surface = tok
        .split_once('{')
        .and_then(|(_, r)| r.split_once('}'))
        .map(|(s, _)| s.to_owned());
    let head_end = tok.find(['(', '[', '{']).unwrap_or(tok.len());
    let headword = tok[..head_end].to_owned();
    let sequence = tok
        .split_once("(#")
        .and_then(|(_, r)| r.split_once(')'))
        .and_then(|(n, _)| n.parse().ok());
    let surface = surface.unwrap_or_else(|| headword.clone());
    (headword, surface, sequence)
}

/// Full-width ASCII (３つ in `B:` lines, 3つ in the sentence) as ASCII,
/// char for char, so spans stay the same.
fn narrow(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// The corpus keeps some particles with the word before (私{私の},
/// 有名{有名な}); a breakdown shows them apart, so the gold does too. A
/// noun with する stays one word (勉強{勉強した}), as the segmenter reads it. Returns the word's own length in chars and the
/// headword of what follows, when the token splits.
fn glued(headword: &str, surface: &str) -> Option<(usize, &'static str)> {
    let rest = surface.strip_prefix(headword).filter(|r| !r.is_empty())?;
    let own = headword.chars().count();
    match rest {
        "の" => Some((own, "の")),
        "な" => Some((own, "な")),
        "に" => Some((own, "に")),
        "と" => Some((own, "と")),
        _ => None,
    }
}

/// The `B:` tokens of `sentence` with their char spans, found left to
/// right; `None` when one can't be found (the line doesn't match its
/// sentence).
fn align(sentence: &str, b_line: &str) -> Option<Vec<Gold>> {
    let chars: Vec<char> = narrow(sentence).chars().collect();
    let mut at = 0;
    let mut out = Vec::new();
    for tok in b_line.split_whitespace() {
        let (headword, surface, sequence) = parse_token(&narrow(tok));
        let needle: Vec<char> = surface.chars().collect();
        if needle.is_empty() {
            return None;
        }
        let start = (at..=chars.len().checked_sub(needle.len())?)
            .find(|&i| chars[i..i + needle.len()] == needle[..])?;
        // Tokens skip punctuation and the like, and names the corpus
        // leaves out (トム, 田中): katakana, or up to 3 kanji. Any other
        // Japanese text between two tokens means a mismatch.
        let gap: Vec<char> = chars[at..start]
            .iter()
            .copied()
            .filter(|&c| japanese_utils::is_japanese(c))
            .collect();
        let name = gap
            .iter()
            .all(|&c| ('ァ'..='ヺ').contains(&c) || c == 'ー' || c == '・')
            || (gap.len() <= 3 && gap.iter().all(|&c| japanese_utils::is_kanji(c)));
        if !name {
            return None;
        }
        at = start + needle.len();
        if let Some((own, next)) = glued(&headword, &surface) {
            out.push(Gold {
                start,
                len: own,
                headword,
                sequence,
            });
            out.push(Gold {
                start: start + own,
                len: needle.len() - own,
                headword: next.to_owned(),
                sequence: None,
            });
            continue;
        }
        out.push(Gold {
            start,
            len: needle.len(),
            headword,
            sequence,
        });
    }
    Some(out)
}

/// `(A sentence, B line)` pairs of the corpus.
fn corpus() -> Vec<(String, String)> {
    let path = std::env::var("EXAMPLES_UTF")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples.utf").into());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("can't read {path} (set EXAMPLES_UTF): {e}"));
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(l) = lines.next() {
        let Some(a) = l.strip_prefix("A: ") else {
            continue;
        };
        let Some(b) = lines.next().and_then(|l| l.strip_prefix("B: ")) else {
            continue;
        };
        out.push((
            a.split('\t').next().unwrap_or_default().to_owned(),
            b.to_owned(),
        ));
    }
    out
}

#[derive(Default)]
struct Counts {
    gold: usize,
    pred: usize,
    both: usize,
}

impl Counts {
    fn f1(&self) -> f64 {
        let p = self.both as f64 / self.pred.max(1) as f64;
        let r = self.both as f64 / self.gold.max(1) as f64;
        2.0 * p * r / (p + r).max(f64::EPSILON)
    }

    fn line(&self, name: &str) -> String {
        let p = self.both as f64 / self.pred.max(1) as f64;
        let r = self.both as f64 / self.gold.max(1) as f64;
        let f = 2.0 * p * r / (p + r).max(f64::EPSILON);
        format!(
            "{name:<10} precision {:5.1}%  recall {:5.1}%  F1 {:5.1}%",
            p * 100.0,
            r * 100.0,
            f * 100.0
        )
    }
}

fn entry_is(e: &ArchivedWordEntry, g: &Gold) -> bool {
    if let Some(seq) = g.sequence {
        return e.sequence.to_native() == seq;
    }
    e.kanji_forms.iter().any(|k| k.text.as_str() == g.headword)
        || e.reading_forms
            .iter()
            .any(|r| r.text.as_str() == g.headword)
}

/// The words of `sentence` as `segment::segment_with` reads them, with
/// the costs of `SEG_COSTS`.
fn split(sentence: &str) -> Vec<Word> {
    let costs = jmdict_core::segment::Costs::default()
        .with(&std::env::var("SEG_COSTS").unwrap_or_default())
        .unwrap();
    jmdict_core::segment::segment_with(sentence, &costs)
}

fn kana(chars: &[char]) -> bool {
    !chars.is_empty() && chars.iter().all(|&c| japanese_utils::is_kana(c))
}

fn digit(c: char) -> bool {
    c.is_ascii_digit() || ('０'..='９').contains(&c)
}

/// Whether the boundary at `p`, which only one side has, is a matter of
/// convention rather than a wrong split: function words in kana that one
/// side keeps whole (では, という, ことに / に|なる, よう|な, すべて|の),
/// or a split after the stem of a word we read as conjugated (成功|する,
/// 行き|たい). `gold` and `pred` are `(start, len)` spans, the gold's
/// leading digits left out.
fn convention(
    p: usize,
    chars: &[char],
    gold: &[(usize, usize)],
    pred: &[Word],
    gold_has: bool,
) -> bool {
    use jmdict_core::segment::Class as C;
    let text = |s: usize, l: usize| &chars[s..s + l];
    if gold_has {
        // We read one word over it.
        let Some(t) = pred.iter().find(|w| w.start < p && p < w.start + w.len) else {
            return false;
        };
        let surface = text(t.start, t.len);
        let function = |c: &C| {
            matches!(
                c,
                C::Expression | C::Particle | C::Auxiliary | C::Conjunction
            )
        };
        if kana(surface)
            && (function(&t.class)
                || t.entry
                    .is_some_and(|e| jmdict_core::segment::classes(e).iter().any(function)))
        {
            return true;
        }
        let base: Vec<char> = t.base.chars().collect();
        let after = gold.iter().find(|&&(s, _)| s == p);
        // する and 来る change their first kana (し, き, こ).
        let irregular = ["する", "為る", "来る", "くる"].contains(&t.base.as_str());
        !t.reasons.is_empty()
            && (irregular || base.first() == surface.first())
            && p - t.start + 1 >= base.len()
            && after.is_some_and(|&(s, l)| kana(text(s, l)))
    } else {
        // The corpus reads one word over it.
        let Some(&(gs, gl)) = gold.iter().find(|&&(s, l)| s < p && p < s + l) else {
            return false;
        };
        let inside: Vec<&Word> = pred
            .iter()
            .filter(|w| w.start >= gs && w.start + w.len <= gs + gl)
            .collect();
        let function = kana(text(gs, gl))
            && inside.iter().all(|w| kana(text(w.start, w.len)))
            && inside
                .iter()
                .any(|w| matches!(w.class, C::Particle | C::Auxiliary));
        // A conjugated word and the kana endings after it (探して|いる,
        // 行かなければ|ならない): the corpus keeps some of those whole.
        let over: Vec<&Word> = pred
            .iter()
            .filter(|w| w.start < gs + gl && gs < w.start + w.len)
            .collect();
        let ending = over.len() > 1
            && over
                .first()
                .is_some_and(|w| w.start <= gs && !w.reasons.is_empty())
            && over.iter().skip(1).all(|w| kana(text(w.start, w.len)));
        function || ending
    }
}

/// `spans` with those meeting at a `forgiven` boundary joined.
fn join_at(
    spans: &BTreeSet<(usize, usize)>,
    forgiven: &BTreeSet<usize>,
) -> BTreeSet<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &(s, l) in spans {
        match out.last_mut() {
            Some(last) if last.0 + last.1 == s && forgiven.contains(&s) => last.1 += l,
            _ => out.push((s, l)),
        }
    }
    out.into_iter().collect()
}

#[test]
#[ignore = "needs JMDICT_BIN (a `full` build) and examples.utf"]
fn segmentation_against_the_corpus() {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN to a full jmdict.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("can't read {path}: {e}"));
    jmdict_core::init(&bytes).expect("init: is JMDICT_BIN a `full` build?");

    let corpus = corpus();
    let mut unaligned = 0;
    let mut spans = Counts::default();
    let mut bounds = Counts::default();
    let (mut entry_ok, mut perfect, mut aligned) = (0, 0, 0);
    let (mut loose_spans, mut loose_bounds, mut loose_perfect) =
        (Counts::default(), Counts::default(), 0);
    let mut conventions = 0;
    // Gold surface → how the segmenter split it, with counts.
    let mut misses: HashMap<(String, String), usize> = HashMap::new();
    let mut report = String::new();

    for (sentence, b_line) in &corpus {
        let Some(gold) = align(sentence, b_line) else {
            unaligned += 1;
            continue;
        };
        aligned += 1;
        let chars: Vec<char> = sentence.chars().collect();
        let covered: Vec<bool> = {
            let mut c = vec![false; chars.len()];
            for g in &gold {
                c[g.start..g.start + g.len].fill(true);
            }
            c
        };
        // Words wholly within what the corpus skips (names, punctuation)
        // aren't scored either way.
        let words: Vec<Word> = split(sentence)
            .into_iter()
            .filter(|w| covered[w.start..w.start + w.len].iter().any(|&c| c))
            .collect();
        let pred: Vec<_> = words.iter().map(|w| (w.start, w.len, w.entry)).collect();

        let gold_spans: BTreeSet<(usize, usize)> = gold.iter().map(|g| (g.start, g.len)).collect();
        let pred_spans: BTreeSet<(usize, usize)> = pred.iter().map(|&(s, l, _)| (s, l)).collect();
        spans.gold += gold_spans.len();
        spans.pred += pred_spans.len();
        spans.both += gold_spans.intersection(&pred_spans).count();

        // Boundaries strictly inside a run of gold words: the positions
        // where a split could go either way. Punctuation edges don't count.
        let inside = |p: usize| p > 0 && p < chars.len() && covered[p - 1] && covered[p];
        let edges = |s: &BTreeSet<(usize, usize)>| -> BTreeSet<usize> {
            s.iter()
                .flat_map(|&(st, l)| [st, st + l])
                .filter(|&p| inside(p))
                .collect()
        };
        let (gb, pb) = (edges(&gold_spans), edges(&pred_spans));
        bounds.gold += gb.len();
        bounds.pred += pb.len();
        bounds.both += gb.intersection(&pb).count();
        if gb == pb {
            perfect += 1;
        }

        // The same, conventions aside: the gold's leading digits (１日 is
        // 日 to us), and the boundaries `convention` forgives.
        let gold_trim: BTreeSet<(usize, usize)> = gold_spans
            .iter()
            .map(|&(s, l)| {
                let d = chars[s..s + l]
                    .iter()
                    .take_while(|&&c| digit(c))
                    .count()
                    .min(l - 1);
                (s + d, l - d)
            })
            .collect();
        let gold_list: Vec<(usize, usize)> = gold_trim.iter().copied().collect();
        let gb2 = edges(&gold_trim);
        let forgiven: BTreeSet<usize> = gb2
            .symmetric_difference(&pb)
            .copied()
            .filter(|&p| convention(p, &chars, &gold_list, &words, gb2.contains(&p)))
            .collect();
        let gold_loose = join_at(&gold_trim, &forgiven);
        let pred_loose = join_at(&pred_spans, &forgiven);
        loose_spans.gold += gold_loose.len();
        loose_spans.pred += pred_loose.len();
        loose_spans.both += gold_loose.intersection(&pred_loose).count();
        let (gb3, pb3) = (edges(&gold_loose), edges(&pred_loose));
        loose_bounds.gold += gb3.len();
        loose_bounds.pred += pb3.len();
        loose_bounds.both += gb3.intersection(&pb3).count();
        if gb3 == pb3 {
            loose_perfect += 1;
        }

        for g in &gold {
            let matched = pred.iter().find(|&&(s, l, _)| (s, l) == (g.start, g.len));
            if let Some(&(_, _, Some(e))) = matched {
                if entry_is(e, g) {
                    entry_ok += 1;
                }
                continue;
            }
            // A convention only: the word meets our split once both are
            // read conventions aside.
            let d = chars[g.start..g.start + g.len]
                .iter()
                .take_while(|&&c| digit(c))
                .count()
                .min(g.len - 1);
            let (ts, te) = (g.start + d, g.start + g.len);
            let in_loose = |spans: &BTreeSet<(usize, usize)>| {
                spans
                    .iter()
                    .find(|&&(s, l)| s <= ts && te <= s + l)
                    .copied()
            };
            if let Some(span) = in_loose(&gold_loose)
                && pred_loose.contains(&span)
            {
                conventions += 1;
                continue;
            }
            let surface: String = chars[g.start..g.start + g.len].iter().collect();
            // How the segmenter cut the stretch from this word's start to
            // its end, extended to the predicted words it overlaps.
            let end = g.start + g.len;
            let split: Vec<String> = pred
                .iter()
                .filter(|&&(s, l, _)| s < end && s + l > g.start)
                .map(|&(s, l, _)| chars[s..s + l].iter().collect())
                .collect();
            let split = if split.is_empty() {
                "(nothing)".to_owned()
            } else {
                split.join("|")
            };
            writeln!(report, "{surface}\t{split}\t{sentence}").unwrap();
            *misses.entry((surface, split)).or_default() += 1;
        }
    }

    eprintln!(
        "\n{} sentences, {aligned} aligned, {unaligned} skipped (B line doesn't match)",
        corpus.len()
    );
    eprintln!("{}", spans.line("words"));
    eprintln!("{}", bounds.line("boundaries"));
    eprintln!(
        "right entry on matched words: {:.1}%   sentences split exactly as the corpus: {:.1}%",
        entry_ok as f64 * 100.0 / spans.both.max(1) as f64,
        perfect as f64 * 100.0 / aligned.max(1) as f64
    );
    eprintln!(
        "conventions aside: words F1 {:.1}%, boundaries F1 {:.1}%, exact {:.1}% ({conventions} corpus words differ by convention only)",
        loose_spans.f1() * 100.0,
        loose_bounds.f1() * 100.0,
        loose_perfect as f64 * 100.0 / aligned.max(1) as f64
    );
    let mut top: Vec<_> = misses.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    eprintln!("\ncommonest disagreements (corpus word → our split):");
    for ((surface, split), n) in top.iter().take(60) {
        eprintln!("{n:6}  {surface} → {split}");
    }
    if let Ok(out) = std::env::var("SEG_REPORT") {
        std::fs::write(&out, report).unwrap();
        eprintln!("\nevery disagreement written to {out}");
    }
}

#[test]
#[ignore = "debug: SEG_WORDS=彼の,では cargo test … seg_debug -- --ignored --nocapture"]
fn seg_debug() {
    let path = std::env::var("JMDICT_BIN").expect("set JMDICT_BIN");
    jmdict_core::init(&std::fs::read(&path).unwrap()).unwrap();
    for w in std::env::var("SEG_WORDS").unwrap_or_default().split(',') {
        for e in jmdict_core::lookup(w) {
            let pos = format!("{:?}", jmdict_core::segment::classes(e));
            let pri = |t: &str| {
                e.kanji_forms
                    .iter()
                    .find(|k| k.text.as_str() == t)
                    .map(|k| k.priorities.len())
                    .or_else(|| {
                        e.reading_forms
                            .iter()
                            .find(|r| r.text.as_str() == t)
                            .map(|r| r.priorities.len())
                    })
            };
            eprintln!("{w}: {} pri={:?} {pos}", e.headword(), pri(w));
        }
    }
    // SEG_AT=文:3 lists the candidates at char 3 of 文, with their costs.
    for spec in std::env::var("SEG_AT").unwrap_or_default().split(',') {
        let Some((s, at)) = spec.rsplit_once(':') else {
            continue;
        };
        let at: usize = at.parse().unwrap();
        let costs = jmdict_core::segment::Costs::default()
            .with(&std::env::var("SEG_COSTS").unwrap_or_default())
            .unwrap();
        let mut c = jmdict_core::segment::candidates_at(s, at, &costs);
        c.sort_by_key(|x| (x.0, x.2));
        for (len, class, cost, head) in c {
            let t: String = s.chars().skip(at).take(len).collect();
            eprintln!("{s}@{at} {t} {head} {class:?} {cost}");
        }
    }
    for s in std::env::var("SEG_SENTENCES")
        .unwrap_or_default()
        .split(',')
    {
        let costs = jmdict_core::segment::Costs::default()
            .with(&std::env::var("SEG_COSTS").unwrap_or_default())
            .unwrap();
        let words: Vec<String> = jmdict_core::segment::segment_with(s, &costs)
            .iter()
            .map(|w| {
                let t: String = s.chars().skip(w.start).take(w.len).collect();
                format!("{t}[{:?}]", w.class)
            })
            .collect();
        eprintln!("{}", words.join(" "));
    }
}
