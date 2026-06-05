use anyhow::{Context, Result};
use jmdict_types::{Gloss, KanjiElement, PartOfSpeech, ReadingElement, Sense, WordEntry};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::Cursor;
use std::path::Path;

pub fn parse_jmdict(path: &Path) -> Result<Vec<WordEntry>> {
    let raw = std::fs::read(path).with_context(|| format!("Failed to open {:?}", path))?;
    parse_jmdict_bytes(&raw)
}

pub fn parse_jmdict_bytes(raw: &[u8]) -> Result<Vec<WordEntry>> {
    let preprocessed = strip_custom_entities(raw);

    let mut reader = Reader::from_reader(Cursor::new(preprocessed));
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut buf = Vec::new();
    let mut b: EntryBuilder = EntryBuilder::default();
    let mut in_entry = false;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Ctx {
        Root,
        EntSeq,
        KanjiElement,
        KeB,
        KeInf,
        KePri,
        ReadingElement,
        ReB,
        ReNokanji,
        ReRestr,
        ReInf,
        RePri,
        Sense,
        Pos,
        Gloss,
        Xref,
        Ant,
        Field,
        Misc,
        SInf,
        Dial,
    }

    let mut ctx = Ctx::Root;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => match e.name().as_ref() {
                b"entry" => {
                    b = EntryBuilder::default();
                    in_entry = true;
                }
                b"k_ele" if in_entry => ctx = Ctx::KanjiElement,
                b"keb" if in_entry => ctx = Ctx::KeB,
                b"ke_inf" if in_entry => ctx = Ctx::KeInf,
                b"ke_pri" if in_entry => ctx = Ctx::KePri,
                b"r_ele" if in_entry => ctx = Ctx::ReadingElement,
                b"reb" if in_entry => ctx = Ctx::ReB,
                b"re_nokanji" if in_entry => {
                    #[cfg(feature = "full")]
                    if let Some(r) = b.current_reading.as_mut() {
                        r.no_kanji = true;
                    }
                    ctx = Ctx::ReNokanji;
                }
                b"re_restr" if in_entry => ctx = Ctx::ReRestr,
                b"re_inf" if in_entry => ctx = Ctx::ReInf,
                b"re_pri" if in_entry => ctx = Ctx::RePri,
                b"sense" if in_entry => {
                    let inherited_pos = b
                        .senses
                        .last()
                        .map(|s: &Sense| s.pos.clone())
                        .unwrap_or_default();
                    b.current_sense_has_explicit_pos = false;
                    b.senses.push(Sense {
                        pos: inherited_pos,
                        ..Default::default()
                    });
                    ctx = Ctx::Sense;
                }
                b"pos" if in_entry => ctx = Ctx::Pos,
                b"gloss" if in_entry => {
                    b.pending_lang = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| {
                            let k = a.key.as_ref();
                            k == b"xml:lang" || k.ends_with(b":lang")
                        })
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                        .unwrap_or_else(|| "eng".to_string());
                    #[cfg(feature = "full")]
                    {
                        b.pending_gtype = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"g_type")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                    }
                    ctx = Ctx::Gloss;
                }
                b"xref" if in_entry => ctx = Ctx::Xref,
                b"ant" if in_entry => ctx = Ctx::Ant,
                b"field" if in_entry => ctx = Ctx::Field,
                b"misc" if in_entry => ctx = Ctx::Misc,
                b"s_inf" if in_entry => ctx = Ctx::SInf,
                b"dial" if in_entry => ctx = Ctx::Dial,
                b"ent_seq" if in_entry => ctx = Ctx::EntSeq,
                _ => {}
            },

            Event::End(e) => match e.name().as_ref() {
                b"entry" => {
                    in_entry = false;
                    ctx = Ctx::Root;
                    if !b.senses.is_empty() {
                        entries.push(WordEntry {
                            sequence: b.sequence,
                            kanji_forms: b.kanji_forms.clone(),
                            reading_forms: b.reading_forms.clone(),
                            senses: b.senses.clone(),
                        });
                    }
                }
                b"k_ele" => {
                    if let Some(k) = b.current_kanji.take() {
                        b.kanji_forms.push(k);
                    }
                    ctx = Ctx::Root;
                }
                b"r_ele" => {
                    if let Some(r) = b.current_reading.take() {
                        b.reading_forms.push(r);
                    }
                    ctx = Ctx::Root;
                }
                b"sense" => {
                    ctx = Ctx::Root;
                }
                b"keb" | b"ke_inf" | b"ke_pri" => {
                    ctx = Ctx::KanjiElement;
                }
                b"reb" | b"re_nokanji" | b"re_restr" | b"re_inf" | b"re_pri" => {
                    ctx = Ctx::ReadingElement;
                }
                b"pos" | b"gloss" | b"xref" | b"ant" | b"field" | b"misc" | b"s_inf" | b"dial" => {
                    ctx = Ctx::Sense;
                }
                b"ent_seq" => {
                    ctx = Ctx::Root;
                }
                _ => {}
            },

            Event::Text(e) => {
                if !in_entry {
                    continue;
                }
                let raw = e.unescape()?.into_owned();
                let text = strip_entity_markers(&raw);

                match ctx {
                    Ctx::EntSeq => b.sequence = text.parse().unwrap_or(0),

                    Ctx::KeB => {
                        b.current_kanji = Some(KanjiElement::from_text(text));
                    }
                    Ctx::KeInf => {
                        if let Some(k) = &mut b.current_kanji {
                            use jmdict_types::KanjiInf;
                            if let Some(inf) = KanjiInf::from_tag(text) {
                                k.info.push(inf);
                            }
                        }
                    }
                    Ctx::KePri => {
                        if let Some(k) = &mut b.current_kanji {
                            use jmdict_types::Freq;
                            if let Some(feq) = Freq::from_tag(text) {
                                k.priorities.push(feq);
                            }
                        }
                    }
                    Ctx::ReB => {
                        b.current_reading = Some(ReadingElement::from_reading(text));
                    }
                    #[cfg(feature = "full")]
                    Ctx::ReRestr => {
                        if let Some(r) = &mut b.current_reading {
                            r.restricted_to.push(text.into());
                        }
                    }
                    #[cfg(feature = "full")]
                    Ctx::ReInf => {
                        if let Some(r) = &mut b.current_reading {
                            r.info.push(text.into());
                        }
                    }
                    Ctx::RePri => {
                        if let Some(r) = &mut b.current_reading {
                            use jmdict_types::Freq;
                            if let Some(feq) = Freq::from_tag(text) {
                                r.priorities.push(feq);
                            }
                        }
                    }

                    Ctx::Pos => {
                        let first_pos = !b.current_sense_has_explicit_pos;
                        if let Some(sense) = b.senses.last_mut() {
                            if first_pos {
                                sense.pos.clear();
                            }
                            let pos = PartOfSpeech::from_entity(text);
                            if pos != PartOfSpeech::Unknown {
                                sense.pos.push(pos);
                            }
                        }
                        b.current_sense_has_explicit_pos = true;
                    }
                    Ctx::Gloss => {
                        if let Some(sense) = b.senses.last_mut()
                            && b.pending_lang == "eng"
                        {
                            sense.glosses.push(Gloss::new(
                                text,
                                cfg_select! {
                                feature = "full" => b.pending_gtype.clone().map(Into::into),
                                _=> None
                                },
                            ));
                        }
                    }
                    #[cfg(feature = "full")]
                    Ctx::Xref => {
                        if let Some(s) = b.senses.last_mut() {
                            s.xrefs.push(text.into());
                        }
                    }
                    #[cfg(feature = "full")]
                    Ctx::Ant => {
                        if let Some(s) = b.senses.last_mut() {
                            s.antonyms.push(text.into());
                        }
                    }
                    #[cfg(feature = "full")]
                    Ctx::Field => {
                        if let Some(s) = b.senses.last_mut() {
                            use jmdict_types::Field;
                            if let Some(field) = Field::from_tag(text) {
                                s.fields.push(field);
                            }
                        }
                    }
                    Ctx::Misc => {
                        if let Some(s) = b.senses.last_mut() {
                            use jmdict_types::Misc;
                            if let Some(misc) = Misc::from_tag(text) {
                                s.misc.push(misc);
                            }
                        }
                    }
                    #[cfg(feature = "full")]
                    Ctx::SInf => {
                        if let Some(s) = b.senses.last_mut() {
                            s.info.push(text.into());
                        }
                    }
                    #[cfg(feature = "full")]
                    Ctx::Dial => {
                        if let Some(s) = b.senses.last_mut() {
                            s.dialects.push(text.into());
                        }
                    }
                    _ => {}
                }
            }

            // Flush pending elements on End events
            Event::Empty(e) if in_entry && e.name().as_ref() == b"re_nokanji" => {
                #[cfg(feature = "full")]
                if let Some(r) = b.current_reading.as_mut() {
                    r.no_kanji = true;
                }
            }

            Event::Eof => break,
            _ => {}
        }

        buf.clear();
    }

    Ok(entries)
}

fn strip_entity_markers(s: &str) -> &str {
    s.trim_start_matches('&').trim_end_matches(';')
}

/// JMdict uses hundreds of custom XML entities (e.g. `&v5b;`, `&unc;`) declared
/// in its DOCTYPE. quick-xml won't resolve them without a DTD processor.
/// This function scans the raw bytes and replaces every `&name;` that isn't a
/// standard XML entity (`amp`, `lt`, `gt`, `apos`, `quot`) with just `name`,
/// leaving numeric references (`&#...;`) and standard entities untouched.
fn strip_custom_entities(input: &[u8]) -> Vec<u8> {
    const STANDARD: &[&[u8]] = &[b"amp", b"lt", b"gt", b"apos", b"quot"];

    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        if input[i] != b'&' {
            out.push(input[i]);
            i += 1;
            continue;
        }

        // Numeric character reference &#...; — leave as-is.
        if i + 1 < input.len() && input[i + 1] == b'#' {
            out.push(input[i]);
            i += 1;
            continue;
        }

        // Find the closing ';'. If it's missing or too far, pass through as-is.
        let name_start = i + 1;
        let mut end = name_start;
        while end < input.len() && input[end] != b';' && input[end] != b'<' && input[end] != b'&' {
            end += 1;
        }

        if end >= input.len() || input[end] != b';' {
            // Not a well-formed reference — emit the '&' verbatim.
            out.push(b'&');
            i += 1;
            continue;
        }

        let name = &input[name_start..end];
        if STANDARD.contains(&name) {
            // Standard entity — keep `&name;` intact for quick-xml.
            out.extend_from_slice(&input[i..=end]);
        } else {
            // Custom entity — emit just the name so quick-xml sees plain text.
            out.extend_from_slice(name);
        }
        i = end + 1;
    }

    out
}

#[derive(Default, Clone)]
struct EntryBuilder {
    sequence: u32,
    kanji_forms: Vec<KanjiElement>,
    reading_forms: Vec<ReadingElement>,
    senses: Vec<Sense>,
    current_kanji: Option<KanjiElement>,
    current_reading: Option<ReadingElement>,
    current_sense_has_explicit_pos: bool,
    pending_lang: String,
    #[cfg(feature = "full")]
    pending_gtype: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_ENTRY: &str = r#"<?xml version="1.0"?>
<JMdict>
<entry>
<ent_seq>1000001</ent_seq>
<k_ele><keb>飲む</keb></k_ele>
<k_ele><keb>呑む</keb></k_ele>
<r_ele><reb>のむ</reb></r_ele>
<sense>
<pos>&v5m;</pos>
<pos>&vt;</pos>
<gloss>to drink</gloss>
<gloss>to swallow</gloss>
</sense>
</entry>
</JMdict>"#;

    #[test]
    fn parses_simple_entry_kanji_reading_senses() {
        let entries = parse_jmdict_bytes(SIMPLE_ENTRY.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.sequence, 1000001);
        assert_eq!(e.kanji_forms.len(), 2);
        assert_eq!(e.kanji_forms[0].text, "飲む");
        assert_eq!(e.kanji_forms[1].text, "呑む");
        assert_eq!(e.reading_forms.len(), 1);
        assert_eq!(e.reading_forms[0].text, "のむ");
        assert_eq!(e.senses.len(), 1);
        assert_eq!(
            e.senses[0].pos,
            vec![PartOfSpeech::VerbGodanMu, PartOfSpeech::VerbTransitive]
        );
        let glosses: Vec<&str> = e.senses[0]
            .glosses
            .iter()
            .map(|g| g.text.as_str())
            .collect();
        assert_eq!(glosses, vec!["to drink", "to swallow"]);
    }

    /// JMdict carries POS forward when subsequent `<sense>` elements omit it.
    /// A sense with no explicit POS should inherit from the previous one;
    /// a later sense with an explicit POS should *replace*, not append.
    #[test]
    fn pos_inherits_then_replaces() {
        let xml = br#"<JMdict>
<entry>
<ent_seq>1</ent_seq>
<r_ele><reb>x</reb></r_ele>
<sense><pos>&n;</pos><gloss>thing</gloss></sense>
<sense><gloss>another sense</gloss></sense>
<sense><pos>&adv;</pos><gloss>thirdly</gloss></sense>
</entry>
</JMdict>"#;
        let entries = parse_jmdict_bytes(xml).unwrap();
        let s = &entries[0].senses;
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].pos, vec![PartOfSpeech::Noun]);
        assert_eq!(s[1].pos, vec![PartOfSpeech::Noun], "sense w/o POS inherits");
        assert_eq!(
            s[2].pos,
            vec![PartOfSpeech::Adverb],
            "explicit POS replaces inheritance"
        );
    }

    /// Non-English glosses (xml:lang != "eng") should be filtered out.
    #[test]
    fn drops_non_english_glosses() {
        let xml = br#"<JMdict>
<entry>
<ent_seq>1</ent_seq>
<r_ele><reb>x</reb></r_ele>
<sense>
<pos>&n;</pos>
<gloss xml:lang="eng">cat</gloss>
<gloss xml:lang="fre">chat</gloss>
<gloss xml:lang="ger">Katze</gloss>
</sense>
</entry>
</JMdict>"#;
        let entries = parse_jmdict_bytes(xml).unwrap();
        let glosses: Vec<&str> = entries[0].senses[0]
            .glosses
            .iter()
            .map(|g| g.text.as_str())
            .collect();
        assert_eq!(glosses, vec!["cat"]);
    }

    /// Entries with no senses must be dropped — the FST relies on entry presence
    /// implying at least one gloss.
    #[test]
    fn skips_entries_with_no_senses() {
        let xml = br#"<JMdict>
<entry><ent_seq>1</ent_seq><r_ele><reb>x</reb></r_ele></entry>
<entry>
<ent_seq>2</ent_seq>
<r_ele><reb>y</reb></r_ele>
<sense><pos>&n;</pos><gloss>second</gloss></sense>
</entry>
</JMdict>"#;
        let entries = parse_jmdict_bytes(xml).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, 2);
    }

    #[test]
    fn strip_custom_entities_replaces_unknown_keeps_standard_and_numeric() {
        let input = b"&v5b; &amp; &#x6587; &unknown; bare & end";
        let out = strip_custom_entities(input);
        let s = std::str::from_utf8(&out).unwrap();
        // Custom entities lose the `&` and `;` markers (so quick-xml sees them as text).
        assert!(s.contains("v5b "));
        assert!(s.contains("unknown "));
        // Standard XML entities stay intact for quick-xml to resolve.
        assert!(s.contains("&amp;"));
        // Numeric character references stay intact.
        assert!(s.contains("&#x6587;"));
        // A lone `&` without a closing `;` is emitted verbatim.
        assert!(s.contains("bare & end"));
    }

    #[test]
    fn parses_multiple_entries() {
        let xml = br#"<JMdict>
<entry><ent_seq>1</ent_seq><r_ele><reb>a</reb></r_ele>
<sense><pos>&n;</pos><gloss>one</gloss></sense></entry>
<entry><ent_seq>2</ent_seq><r_ele><reb>b</reb></r_ele>
<sense><pos>&n;</pos><gloss>two</gloss></sense></entry>
<entry><ent_seq>3</ent_seq><r_ele><reb>c</reb></r_ele>
<sense><pos>&n;</pos><gloss>three</gloss></sense></entry>
</JMdict>"#;
        let entries = parse_jmdict_bytes(xml).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}
