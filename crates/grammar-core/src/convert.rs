//! Yomitan grammar dictionary ZIP → `grammar.bin`.
//!
//! Written against Bee's Ultimate Grammar Dictionary, whose structured
//! content labels every part of a card with a `data` role (`meaning`,
//! `sourceBlock`, `prose`, `example`…). The converter keeps what those roles
//! say and drops Yomitan's layout. Anything it doesn't recognise is walked
//! into, so its text survives as plain paragraphs.
//!
//! Runs on the user's device: the dictionary's content is the sources'
//! own, and Yomeru never redistributes it.

use std::io::{Cursor, Read};

use anyhow::{Context, bail};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::format;
use crate::types::{
    Block, Example, GrammarEntry, GrammarMeta, ParagraphKind, Rich, Sense, SourceBlock, Span,
};

/// What a conversion produced, for the import screen.
#[derive(Debug, Clone)]
pub struct Converted {
    pub bytes: Vec<u8>,
    pub meta: GrammarMeta,
    pub entries: usize,
}

/// Convert a Yomitan dictionary ZIP (format 3) into `grammar.bin` bytes.
pub fn convert(zip_bytes: &[u8]) -> anyhow::Result<Converted> {
    let mut converter = Converter::new(zip_bytes.to_vec())?;
    while converter.step()? {}
    converter.finish()
}

/// [`convert`] in steps of one term bank, so a caller on a UI thread can
/// show progress and let the page repaint between them:
///
/// ```ignore
/// let mut c = Converter::new(zip)?;
/// while c.step()? { show(c.done(), c.total()); }
/// let converted = c.finish()?;
/// ```
pub struct Converter {
    archive: zip::ZipArchive<Cursor<Vec<u8>>>,
    meta: GrammarMeta,
    /// Term bank file names, in the dictionary's order.
    banks: Vec<String>,
    done: usize,
    packed: Vec<format::Packed>,
}

impl Converter {
    /// Opens the ZIP and reads its index. Fails on anything but a format-3
    /// Yomitan dictionary with term banks.
    pub fn new(zip: Vec<u8>) -> anyhow::Result<Self> {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).context("not a ZIP archive")?;

        let index: Index = serde_json::from_slice(&read_file(&mut archive, "index.json")?)
            .context("index.json isn't a Yomitan index")?;
        if index.format.or(index.version) != Some(3) {
            bail!("unsupported Yomitan dictionary format (only format 3 is)");
        }
        let meta = GrammarMeta {
            title: index.title,
            revision: index.revision,
            attribution: index.attribution,
            url: index.url,
            index_url: index.index_url,
            download_url: index.download_url,
        };

        // term_bank_1.json, term_bank_2.json… in numeric order, which is the
        // dictionary's own order.
        let mut banks: Vec<(u32, String)> = archive
            .file_names()
            .filter_map(|name| {
                let n = name.strip_prefix("term_bank_")?.strip_suffix(".json")?;
                Some((n.parse().ok()?, name.to_owned()))
            })
            .collect();
        banks.sort();
        if banks.is_empty() {
            bail!("the dictionary has no term banks");
        }
        Ok(Self {
            archive,
            meta,
            banks: banks.into_iter().map(|(_, name)| name).collect(),
            done: 0,
            packed: Vec::new(),
        })
    }

    pub fn meta(&self) -> &GrammarMeta {
        &self.meta
    }

    /// Steps the conversion takes: one per term bank.
    pub fn total(&self) -> usize {
        self.banks.len()
    }

    /// Steps done so far.
    pub fn done(&self) -> usize {
        self.done
    }

    /// Converts and compresses the next term bank. Returns whether any
    /// remain.
    pub fn step(&mut self) -> anyhow::Result<bool> {
        let Some(name) = self.banks.get(self.done).cloned() else {
            return Ok(false);
        };
        // One bank at a time: parsed JSON is several times its text's size.
        let rows: Vec<Row> = serde_json::from_slice(&read_file(&mut self.archive, &name)?)
            .with_context(|| format!("{name} isn't a Yomitan term bank"))?;
        for row in rows {
            self.packed.push(format::pack(&entry_from_row(row))?);
        }
        self.done += 1;
        Ok(self.done < self.banks.len())
    }

    /// Writes `grammar.bin` from the converted banks. Converts any left
    /// first.
    pub fn finish(mut self) -> anyhow::Result<Converted> {
        while self.step()? {}
        let bytes = format::write(&self.meta, &self.packed)?;
        Ok(Converted {
            bytes,
            meta: self.meta,
            entries: self.packed.len(),
        })
    }
}

fn read_file(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    name: &str,
) -> anyhow::Result<Vec<u8>> {
    let mut file = archive
        .by_name(name)
        .with_context(|| format!("the dictionary has no {name}"))?;
    let mut out = Vec::new();
    file.read_to_end(&mut out)
        .with_context(|| format!("couldn't unpack {name}"))?;
    Ok(out)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    #[serde(default)]
    title: String,
    #[serde(default)]
    revision: String,
    format: Option<u8>,
    version: Option<u8>,
    #[serde(default)]
    attribution: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    index_url: String,
    #[serde(default)]
    download_url: String,
}

/// A format-3 term bank row:
/// `[term, reading, definitionTags, rules, score, glossary, sequence, termTags]`.
#[derive(Deserialize)]
struct Row(
    String,
    String,
    IgnoredAny,
    String,
    IgnoredAny,
    Vec<Value>,
    IgnoredAny,
    IgnoredAny,
);

fn entry_from_row(row: Row) -> GrammarEntry {
    let Row(term, reading, _, rules, _, glossary, _, _) = row;
    let mut entry = GrammarEntry {
        term,
        reading,
        rules,
        ..GrammarEntry::default()
    };
    for item in &glossary {
        card(item, &mut entry);
    }
    entry
}

// ---- Structured content access ---------------------------------------------

/// A node's children: `content` may be one node or a list of them.
fn children(node: &Value) -> &[Value] {
    let content = match node {
        Value::Object(o) => o.get("content"),
        _ => None,
    };
    match content {
        Some(Value::Array(a)) => a,
        Some(v) => std::slice::from_ref(v),
        None => &[],
    }
}

fn tag(node: &Value) -> &str {
    node.get("tag").and_then(Value::as_str).unwrap_or("")
}

fn has_role(node: &Value, role: &str) -> bool {
    node.get("data")
        .and_then(Value::as_object)
        .is_some_and(|d| d.contains_key(role))
}

/// The first node, depth first, carrying `role`.
fn find<'a>(node: &'a Value, role: &str) -> Option<&'a Value> {
    if has_role(node, role) {
        return Some(node);
    }
    children(node).iter().find_map(|c| find(c, role))
}

/// All text under a node, whitespace at the ends trimmed.
fn text(node: &Value) -> String {
    fn walk(node: &Value, out: &mut String) {
        match node {
            Value::String(s) => out.push_str(s),
            Value::Object(o) if o.get("type").and_then(Value::as_str) == Some("text") => {
                if let Some(s) = o.get("text").and_then(Value::as_str) {
                    out.push_str(s);
                }
            }
            _ if tag(node) == "rt" || tag(node) == "rp" => {}
            _ => children(node).iter().for_each(|c| walk(c, out)),
        }
    }
    let mut out = String::new();
    walk(node, &mut out);
    out.trim().to_owned()
}

// ---- Card ------------------------------------------------------------------

fn card(node: &Value, entry: &mut GrammarEntry) {
    if has_role(node, "compact") {
        if let Some(n) = find(node, "meaning") {
            entry.meaning = text(n);
        }
        if let Some(n) = find(node, "structure") {
            entry.structure = text(n);
        }
        if let Some(n) = find(node, "jlpt") {
            entry.jlpt = text(n);
        }
    } else if has_role(node, "sourceBlock") {
        entry.sources.push(source(node));
    } else if has_role(node, "crossref") {
        crossref(node, &mut entry.see_also);
    } else if has_role(node, "website") {
        if let Some(href) = first_href(node) {
            entry.website = href;
        }
    } else if let Value::String(s) = node {
        // A plain-text glossary: keep it as the meaning.
        if entry.meaning.is_empty() {
            entry.meaning = s.trim().to_owned();
        }
    } else {
        children(node).iter().for_each(|c| card(c, entry));
    }
}

fn crossref(node: &Value, out: &mut Vec<String>) {
    let before = out.len();
    collect_links(node, out);
    if out.len() == before {
        let t = text(node);
        let t = t.strip_prefix("see ").unwrap_or(&t).trim();
        if !t.is_empty() {
            out.push(t.to_owned());
        }
    }
}

fn collect_links(node: &Value, out: &mut Vec<String>) {
    if tag(node) == "a" {
        let t = text(node);
        if !t.is_empty() {
            out.push(t);
        }
        return;
    }
    children(node).iter().for_each(|c| collect_links(c, out));
}

fn first_href(node: &Value) -> Option<String> {
    if tag(node) == "a" {
        return node.get("href").and_then(Value::as_str).map(str::to_owned);
    }
    children(node).iter().find_map(first_href)
}

// ---- Source block ----------------------------------------------------------

fn source(node: &Value) -> SourceBlock {
    let mut block = SourceBlock::default();
    let mut label = String::new();
    source_walk(node, &mut block, &mut label);
    block
}

fn source_walk(node: &Value, block: &mut SourceBlock, label: &mut String) {
    for child in children(node) {
        if has_role(child, "sourceName") {
            block.name = text(child);
        } else if has_role(child, "sourceLevel") {
            block.level = find(child, "jlpt").map(text).unwrap_or_else(|| {
                let t = text(child);
                t.strip_prefix("JLPT").unwrap_or(&t).trim().to_owned()
            });
        } else if has_role(child, "senseLabel") {
            *label = text(child);
        } else if has_role(child, "sense") {
            let mut blocks = Vec::new();
            blocks_of(child, ParagraphKind::Plain, &mut blocks);
            block.senses.push(Sense {
                label: std::mem::take(label),
                blocks,
            });
        } else if child.is_object() {
            source_walk(child, block, label);
        }
    }
}

// ---- Blocks ----------------------------------------------------------------

const BLOCK_TAGS: &[&str] = &[
    "div", "ul", "ol", "li", "details", "summary", "table", "thead", "tbody", "tfoot", "tr", "td",
    "th",
];

fn is_block(node: &Value) -> bool {
    BLOCK_TAGS.contains(&tag(node))
}

/// The paragraph kind a node's roles name, most specific first.
fn kind_of(node: &Value) -> Option<ParagraphKind> {
    const ROLES: &[(&str, ParagraphKind)] = &[
        ("exampleAnnotation", ParagraphKind::Annotation),
        ("exampleDerivation", ParagraphKind::Derivation),
        ("proseHeading", ParagraphKind::Heading),
        ("exampleLabel", ParagraphKind::ExampleLabel),
        ("inlineExample", ParagraphKind::InlineExample),
        ("proseTranslation", ParagraphKind::Translation),
    ];
    ROLES
        .iter()
        .find(|(role, _)| has_role(node, role))
        .map(|&(_, kind)| kind)
}

/// The blocks under `node`. Consecutive inline children make one paragraph
/// of `kind`; block children are walked into, taking their own kind.
fn blocks_of(node: &Value, kind: ParagraphKind, out: &mut Vec<Block>) {
    let mut run: Rich = Vec::new();
    for child in children(node) {
        if has_role(child, "illustration") {
            continue;
        }
        if has_role(child, "examples") {
            flush(&mut run, kind, out);
            examples(child, out);
        } else if has_role(child, "patterns") {
            flush(&mut run, kind, out);
            let items: Vec<Rich> = children(child)
                .iter()
                .map(rich)
                .flat_map(|r| match plain_text(&r) {
                    // "2. V-ばよかったa. V-ばよかった": a pattern and its first
                    // sub-pattern, glued upstream.
                    Some(t) => subheadings(&t)
                        .into_iter()
                        .map(|piece| {
                            vec![Span::Text {
                                text: piece.trim().to_owned(),
                            }]
                        })
                        .collect(),
                    None => vec![r],
                })
                .filter(|r| !is_blank(r))
                .collect();
            if !items.is_empty() {
                out.push(Block::Patterns { items });
            }
        } else if has_role(child, "example") {
            flush(&mut run, kind, out);
            examples(&serde_json::json!({ "content": [child] }), out);
        } else if is_block(child) {
            flush(&mut run, kind, out);
            let own = kind_of(child).unwrap_or(kind);
            if children(child).iter().any(is_block) {
                blocks_of(child, own, out);
            } else {
                push_paragraph(rich(child), own, out);
            }
        } else {
            inline(child, &mut run);
        }
    }
    flush(&mut run, kind, out);
}

fn flush(run: &mut Rich, kind: ParagraphKind, out: &mut Vec<Block>) {
    push_paragraph(std::mem::take(run), kind, out);
}

fn push_paragraph(mut text: Rich, kind: ParagraphKind, out: &mut Vec<Block>) {
    trim(&mut text);
    if is_blank(&text) {
        return;
    }
    let Some(plain) = plain_text(&text) else {
        out.push(Block::Paragraph { kind, text });
        return;
    };
    // Sub-senses some sources run into one paragraph: "…ought to have been.
    // b. V-なければよかった In the forms…" becomes a heading and its text.
    let mut pieces = subheadings(&plain).into_iter();
    if let Some(first) = pieces.next() {
        push_text_paragraph(first, kind, out);
    }
    for piece in pieces {
        match heading_end(piece) {
            Some(end) => {
                push_text_paragraph(
                    piece.get(..end).unwrap_or(piece),
                    ParagraphKind::Heading,
                    out,
                );
                push_text_paragraph(piece.get(end..).unwrap_or_default(), kind, out);
            }
            None => push_text_paragraph(piece, kind, out),
        }
    }
}

fn push_text_paragraph(text: &str, kind: ParagraphKind, out: &mut Vec<Block>) {
    let text = text.trim();
    if !text.is_empty() {
        out.push(Block::Paragraph {
            kind,
            text: vec![Span::Text {
                text: text.to_owned(),
            }],
        });
    }
}

/// The text of a run made only of plain text and line breaks; `None` when
/// it carries styling that splitting would lose.
fn plain_text(rich: &[Span]) -> Option<String> {
    rich.iter()
        .all(|s| matches!(s, Span::Text { .. } | Span::LineBreak))
        .then(|| crate::types::plain(rich))
}

/// `text` cut before each glued sub-heading: a letter a–h and a dot, after
/// anything but a letter or digit, then grammar notation (`V-`, `N`,
/// `A-`, `Ｖ－`…). "…been. b. V-なければよかった In…" → ["…been. ",
/// "b. V-なければよかった In…"]. The text's own start is never cut.
fn subheadings(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let mut cuts = Vec::new();
    for (i, &(byte, c)) in chars.iter().enumerate().skip(1) {
        if !('a'..='h').contains(&c) || at(i + 1) != Some('.') {
            continue;
        }
        if at(i - 1).is_some_and(|p| p.is_ascii_alphanumeric()) {
            continue;
        }
        let mut j = i + 2;
        if at(j) == Some(' ') {
            j += 1;
        }
        let notation = at(j).is_some_and(|n| "VNAＶＮＡ".contains(n));
        let follows = at(j + 1).is_some_and(|f| {
            f == '-'
                || f == '－'
                || f.is_ascii_digit()
                || ('１'..='９').contains(&f)
                || !f.is_ascii()
        });
        if notation && follows {
            cuts.push(byte);
        }
    }
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut start = 0;
    for cut in cuts {
        out.extend(text.get(start..cut));
        start = cut;
    }
    out.extend(text.get(start..));
    out
}

/// Where a sub-heading's notation ends and its English text begins: the
/// first space followed by a capital letter. `None` when no English text
/// follows ("c. V-ようか〈invitation〉").
fn heading_end(piece: &str) -> Option<usize> {
    // Past the marker ("b. ") and the notation's first char (the "V" of
    // "V-…", itself a capital after a space).
    let chars = piece
        .char_indices()
        .skip(2)
        .skip_while(|&(_, c)| c == ' ')
        .skip(1);
    let mut prev = None;
    for (i, c) in chars {
        if prev == Some(' ') && c.is_ascii_uppercase() {
            return Some(i);
        }
        prev = Some(c);
    }
    None
}

fn examples(node: &Value, out: &mut Vec<Block>) {
    let items: Vec<Example> = children(node)
        .iter()
        .filter(|c| has_role(c, "example"))
        .map(|li| {
            let mut ja = find(li, "ja").map(rich).unwrap_or_default();
            trim(&mut ja);
            Example {
                ja,
                en: find(li, "en").map(text).unwrap_or_default(),
            }
        })
        .filter(|e| !is_blank(&e.ja))
        .collect();
    if !items.is_empty() {
        out.push(Block::Examples { items });
    }
}

// ---- Inline text -----------------------------------------------------------

fn rich(node: &Value) -> Rich {
    let mut out = Vec::new();
    for child in children(node) {
        inline(child, &mut out);
    }
    out
}

fn inline(node: &Value, out: &mut Rich) {
    match node {
        Value::String(s) => push_text(out, s),
        Value::Object(o) if o.get("type").and_then(Value::as_str) == Some("text") => {
            if let Some(s) = o.get("text").and_then(Value::as_str) {
                push_text(out, s);
            }
        }
        Value::Object(_) => match tag(node) {
            "br" => out.push(Span::LineBreak),
            "img" | "rt" | "rp" => {}
            "ruby" => {
                let ruby = children(node)
                    .iter()
                    .filter(|c| tag(c) == "rt")
                    .flat_map(children)
                    .map(text)
                    .collect::<String>();
                let base = text(node);
                if ruby.is_empty() {
                    push_text(out, &base);
                } else {
                    out.push(Span::Ruby { text: base, ruby });
                }
            }
            "a" => {
                let text = text(node);
                let query = node
                    .get("href")
                    .and_then(Value::as_str)
                    .and_then(query_of)
                    .unwrap_or_else(|| text.clone());
                out.push(Span::Link { text, query });
            }
            _ if has_role(node, "hl") => out.push(Span::Highlight { text: text(node) }),
            _ if has_role(node, "proseLabel") => out.push(Span::Label { text: text(node) }),
            _ => {
                // A block nested in inline text: keep its lines apart.
                let block = is_block(node);
                if block && !out.is_empty() {
                    out.push(Span::LineBreak);
                }
                children(node).iter().for_each(|c| inline(c, out));
                if block {
                    out.push(Span::LineBreak);
                }
            }
        },
        _ => {}
    }
}

/// `?query=AながらB&wildcards=off` → `AながらB`.
fn query_of(href: &str) -> Option<String> {
    let q = href.split_once("query=")?.1;
    let q = q.split('&').next().unwrap_or(q);
    Some(percent_decode(q))
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn push_text(out: &mut Rich, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Span::Text { text }) = out.last_mut() {
        text.push_str(s);
    } else {
        out.push(Span::Text { text: s.to_owned() });
    }
}

fn is_blank(rich: &[Span]) -> bool {
    rich.iter().all(|s| s.text().trim().is_empty())
}

/// Drop leading and trailing whitespace and line breaks.
fn trim(rich: &mut Rich) {
    while matches!(rich.last(), Some(Span::LineBreak)) {
        rich.pop();
    }
    while matches!(rich.first(), Some(Span::LineBreak)) {
        rich.remove(0);
    }
    if let Some(Span::Text { text }) = rich.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(Span::Text { text }) = rich.last_mut() {
        *text = text.trim_end().to_owned();
    }
    rich.retain(|s| !matches!(s, Span::Text { text } if text.is_empty()));
}
