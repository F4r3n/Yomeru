//! KanjiVG → each kanji's top-level components.
//!
//! A kanji is a tree of `<g>` groups. Groups with `kvg:element` are
//! components; groups without one only position strokes (left half, top…) and
//! are looked through. A component split around another one (門 around 口)
//! appears as several groups tagged `kvg:part`, and counts once.

use anyhow::{Context, Result};
use kanjidic_types::KanjiPart;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::path::Path;

pub fn parse_kanjivg(path: &Path) -> Result<HashMap<char, Vec<KanjiPart>>> {
    let raw = std::fs::read(path).with_context(|| format!("Failed to open {:?}", path))?;
    parse_kanjivg_bytes(&raw)
}

pub fn parse_kanjivg_bytes(raw: &[u8]) -> Result<HashMap<char, Vec<KanjiPart>>> {
    let mut reader = Reader::from_reader(raw);
    let mut buf = Vec::new();
    let mut out = HashMap::new();

    // The kanji being read; None outside `<kanji>` and inside the stylistic
    // variants (ids like `kvg:kanji_08a9e-Kaisho`), which are skipped.
    let mut literal: Option<char> = None;
    let mut parts: Vec<KanjiPart> = Vec::new();
    // One entry per open `<g>`: whether it lies inside an already-collected
    // component, whose own sub-groups belong to that component.
    let mut groups: Vec<bool> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) if e.name().as_ref() == b"kanji" => {
                literal = attr(&e, b"id").and_then(|id| kanji_from_id(&id));
                parts.clear();
                groups.clear();
            }
            Event::End(e) if e.name().as_ref() == b"kanji" => {
                // A lone part is only some of the strokes (言 lists just 口,
                // its other strokes being ungrouped), so it isn't a split.
                if let Some(k) = literal.take() {
                    if parts.len() >= 2 {
                        out.insert(k, std::mem::take(&mut parts));
                    }
                }
            }
            Event::Start(e) if e.name().as_ref() == b"g" => {
                let inside = visit_group(&e, literal.is_some(), &groups, &mut parts);
                groups.push(inside);
            }
            Event::Empty(e) if e.name().as_ref() == b"g" => {
                visit_group(&e, literal.is_some(), &groups, &mut parts);
            }
            Event::End(e) if e.name().as_ref() == b"g" => {
                groups.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Record `g` as a component if it is one at the top level, and return
/// whether its subtree lies inside a component.
fn visit_group(
    g: &BytesStart<'_>,
    in_kanji: bool,
    groups: &[bool],
    parts: &mut Vec<KanjiPart>,
) -> bool {
    // The outermost group is the kanji itself.
    if !in_kanji || groups.is_empty() {
        return false;
    }
    if groups.last().copied().unwrap_or(false) {
        return true;
    }
    let Some(element) = attr(g, b"kvg:element").and_then(|s| s.chars().next()) else {
        return false;
    };
    let split = attr(g, b"kvg:part").is_some();
    if !(split && parts.iter().any(|p| p.element == element)) {
        let radical = attr(g, b"kvg:radical");
        parts.push(KanjiPart {
            element,
            original: attr(g, b"kvg:original").and_then(|s| s.chars().next()),
            radical: matches!(radical.as_deref(), Some("general" | "tradit")),
            phonetic: attr(g, b"kvg:phon").is_some(),
        });
    }
    true
}

/// `kvg:kanji_08a9e` → 語. Variant ids carry a `-Suffix` and give None.
fn kanji_from_id(id: &str) -> Option<char> {
    let hex = id.strip_prefix("kvg:kanji_")?;
    char::from_u32(u32::from_str_radix(hex, 16).ok()?)
}

fn attr(e: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elements(parts: &[KanjiPart]) -> Vec<char> {
        parts.iter().map(|p| p.element).collect()
    }

    const SAMPLE: &str = r#"<kanjivg xmlns:kvg='http://kanjivg.tagaini.net'>
<kanji id="kvg:kanji_08a9e">
<g id="kvg:08a9e" kvg:element="語">
	<g id="kvg:08a9e-g1" kvg:element="言" kvg:position="left" kvg:radical="general">
		<g id="kvg:08a9e-g2" kvg:element="口">
			<path id="kvg:08a9e-s1" d="M0,0"/>
		</g>
	</g>
	<g id="kvg:08a9e-g3" kvg:element="吾" kvg:position="right" kvg:phon="吾">
		<g id="kvg:08a9e-g4" kvg:element="五">
			<g id="kvg:08a9e-g5" kvg:element="二" kvg:part="1"/>
			<g id="kvg:08a9e-g6" kvg:element="二" kvg:part="2"/>
		</g>
		<g id="kvg:08a9e-g7" kvg:element="口"/>
	</g>
</g>
</kanji>
<kanji id="kvg:kanji_08a9e-Kaisho">
<g id="kvg:08a9e-Kaisho" kvg:element="語">
	<g id="kvg:08a9e-Kaisho-g1" kvg:element="木"/>
</g>
</kanji>
<kanji id="kvg:kanji_06d77">
<g id="kvg:06d77" kvg:element="海">
	<g id="kvg:06d77-g1" kvg:element="氵" kvg:variant="true" kvg:original="水" kvg:position="left" kvg:radical="general"/>
	<g id="kvg:06d77-g2" kvg:element="毎" kvg:position="right" kvg:phon="毎"/>
</g>
</kanji>
<kanji id="kvg:kanji_056fd">
<g id="kvg:056fd" kvg:element="国">
	<g id="kvg:056fd-g1" kvg:element="囗" kvg:part="1" kvg:position="kamae" kvg:radical="general"/>
	<g id="kvg:056fd-g2" kvg:position="kamae">
		<g id="kvg:056fd-g3" kvg:element="玉"/>
	</g>
	<g id="kvg:056fd-g4" kvg:element="囗" kvg:part="2" kvg:position="kamae"/>
</g>
</kanji>
<kanji id="kvg:kanji_08a00">
<g id="kvg:08a00" kvg:element="言">
	<path id="kvg:08a00-s1" d="M0,0"/>
	<g id="kvg:08a00-g1" kvg:element="口"/>
</g>
</kanji>
<kanji id="kvg:kanji_04e00">
<g id="kvg:04e00" kvg:element="一">
	<path id="kvg:04e00-s1" d="M0,0"/>
</g>
</kanji>
</kanjivg>"#;

    #[test]
    fn takes_top_level_components_only() {
        let map = parse_kanjivg_bytes(SAMPLE.as_bytes()).unwrap();
        let go = &map[&'語'];
        assert_eq!(elements(go), vec!['言', '吾']);
        assert!(go[0].radical && !go[0].phonetic);
        assert!(!go[1].radical && go[1].phonetic);
    }

    #[test]
    fn stylistic_variants_do_not_override_the_main_entry() {
        let map = parse_kanjivg_bytes(SAMPLE.as_bytes()).unwrap();
        assert_eq!(elements(&map[&'語']), vec!['言', '吾']);
    }

    #[test]
    fn keeps_the_original_of_a_variant_form() {
        let map = parse_kanjivg_bytes(SAMPLE.as_bytes()).unwrap();
        let umi = &map[&'海'];
        assert_eq!(umi[0].element, '氵');
        assert_eq!(umi[0].original, Some('水'));
        assert_eq!(umi[1].original, None);
    }

    #[test]
    fn split_components_count_once_and_positional_groups_are_looked_through() {
        let map = parse_kanjivg_bytes(SAMPLE.as_bytes()).unwrap();
        assert_eq!(elements(&map[&'国']), vec!['囗', '玉']);
    }

    #[test]
    fn kanji_without_components_are_left_out() {
        let map = parse_kanjivg_bytes(SAMPLE.as_bytes()).unwrap();
        assert!(!map.contains_key(&'一'));
    }

    #[test]
    fn a_single_component_is_not_a_split() {
        let map = parse_kanjivg_bytes(SAMPLE.as_bytes()).unwrap();
        assert!(!map.contains_key(&'言'));
    }
}
