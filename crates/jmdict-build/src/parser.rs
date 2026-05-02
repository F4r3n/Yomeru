use anyhow::{Context, Result};
use jmdict_types::{Gloss, KanjiElement, PartOfSpeech, ReadingElement, Sense, WordEntry};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Cursor;
use std::path::Path;

pub fn parse_jmdict(path: &Path) -> Result<Vec<WordEntry>> {
    let raw = std::fs::read(path)
        .with_context(|| format!("Failed to open {:?}", path))?;
    let preprocessed = strip_custom_entities(&raw);

    let mut reader = Reader::from_reader(Cursor::new(preprocessed));
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut buf = Vec::new();
    let mut b: EntryBuilder = EntryBuilder::default();
    let mut in_entry = false;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Ctx {
        Root, EntSeq,
        KanjiElement, KeB, KeInf, KePri,
        ReadingElement, ReB, ReNokanji, ReRestr, ReInf, RePri,
        Sense, Pos, Gloss, Xref, Ant, Field, Misc, SInf, Dial,
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
                b"keb"   if in_entry => ctx = Ctx::KeB,
                b"ke_inf" if in_entry => ctx = Ctx::KeInf,
                b"ke_pri" if in_entry => ctx = Ctx::KePri,
                b"r_ele" if in_entry => ctx = Ctx::ReadingElement,
                b"reb"   if in_entry => ctx = Ctx::ReB,
                b"re_nokanji" if in_entry => {
                    if let Some(r) = b.current_reading.as_mut() { r.no_kanji = true; }
                    ctx = Ctx::ReNokanji;
                }
                b"re_restr" if in_entry => ctx = Ctx::ReRestr,
                b"re_inf"   if in_entry => ctx = Ctx::ReInf,
                b"re_pri"   if in_entry => ctx = Ctx::RePri,
                b"sense" if in_entry => {
                    let inherited_pos = b.senses.last().map(|s: &Sense| s.pos.clone()).unwrap_or_default();
                    b.senses.push(Sense {
                        pos: inherited_pos,
                        glosses: vec![],
                        xrefs: vec![],
                        antonyms: vec![],
                        fields: vec![],
                        misc: vec![],
                        info: vec![],
                        dialects: vec![],
                    });
                    ctx = Ctx::Sense;
                }
                b"pos"   if in_entry => ctx = Ctx::Pos,
                b"gloss" if in_entry => {
                    b.pending_lang = e.attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| {
                            let k = a.key.as_ref();
                            k == b"xml:lang" || k.ends_with(b":lang")
                        })
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                        .unwrap_or_else(|| "eng".to_string());
                    b.pending_gtype = e.attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"g_type")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                    ctx = Ctx::Gloss;
                }
                b"xref"  if in_entry => ctx = Ctx::Xref,
                b"ant"   if in_entry => ctx = Ctx::Ant,
                b"field" if in_entry => ctx = Ctx::Field,
                b"misc"  if in_entry => ctx = Ctx::Misc,
                b"s_inf" if in_entry => ctx = Ctx::SInf,
                b"dial"  if in_entry => ctx = Ctx::Dial,
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
                b"k_ele" => { ctx = Ctx::Root; }
                b"r_ele" => { ctx = Ctx::Root; }
                b"sense" => { ctx = Ctx::Root; }
                b"keb" | b"ke_inf" | b"ke_pri" => { ctx = Ctx::KanjiElement; }
                b"reb" | b"re_nokanji" | b"re_restr" | b"re_inf" | b"re_pri" => {
                    ctx = Ctx::ReadingElement;
                }
                b"pos" | b"gloss" | b"xref" | b"ant" | b"field" | b"misc" | b"s_inf" | b"dial" => {
                    ctx = Ctx::Sense;
                }
                b"ent_seq" => { ctx = Ctx::Root; }
                _ => {}
            },

            Event::Text(e) => {
                if !in_entry { continue; }
                let raw = e.unescape()?.into_owned();
                let text = strip_entity_markers(&raw);

                match ctx {
                    Ctx::EntSeq => b.sequence = text.parse().unwrap_or(0),

                    Ctx::KeB => b.current_kanji = Some(KanjiElement {
                        text: text.to_string(), info: vec![], priorities: vec![],
                    }),
                    Ctx::KeInf => { if let Some(k) = &mut b.current_kanji { k.info.push(text.to_string()); } }
                    Ctx::KePri => { if let Some(k) = &mut b.current_kanji { k.priorities.push(text.to_string()); } }
                    // When we exit k_ele we push — handled in End(k_ele) via flush
                    Ctx::KanjiElement => {
                        // finalize: push pending kanji
                        if let Some(k) = b.current_kanji.take() { b.kanji_forms.push(k); }
                    }

                    Ctx::ReB => b.current_reading = Some(ReadingElement {
                        text: text.to_string(), no_kanji: false,
                        restricted_to: vec![], info: vec![], priorities: vec![],
                    }),
                    Ctx::ReRestr => { if let Some(r) = &mut b.current_reading { r.restricted_to.push(text.to_string()); } }
                    Ctx::ReInf  => { if let Some(r) = &mut b.current_reading { r.info.push(text.to_string()); } }
                    Ctx::RePri  => { if let Some(r) = &mut b.current_reading { r.priorities.push(text.to_string()); } }

                    Ctx::Pos => {
                        if let Some(sense) = b.senses.last_mut() {
                            // New POS found — clear the inherited POS on first explicit tag
                            if sense.pos.iter().all(|p| *p == PartOfSpeech::Unknown) {
                                sense.pos.clear();
                            }
                            let pos = PartOfSpeech::from_entity(text);
                            if pos != PartOfSpeech::Unknown || !sense.pos.contains(&PartOfSpeech::Unknown) {
                                sense.pos.push(pos);
                            }
                        }
                    }
                    Ctx::Gloss => {
                        if let Some(sense) = b.senses.last_mut() {
                            sense.glosses.push(Gloss {
                                text: text.to_string(),
                                lang: b.pending_lang.clone(),
                                gloss_type: b.pending_gtype.clone(),
                            });
                        }
                    }
                    Ctx::Xref  => { if let Some(s) = b.senses.last_mut() { s.xrefs.push(text.to_string()); } }
                    Ctx::Ant   => { if let Some(s) = b.senses.last_mut() { s.antonyms.push(text.to_string()); } }
                    Ctx::Field => { if let Some(s) = b.senses.last_mut() { s.fields.push(text.to_string()); } }
                    Ctx::Misc  => { if let Some(s) = b.senses.last_mut() { s.misc.push(text.to_string()); } }
                    Ctx::SInf  => { if let Some(s) = b.senses.last_mut() { s.info.push(text.to_string()); } }
                    Ctx::Dial  => { if let Some(s) = b.senses.last_mut() { s.dialects.push(text.to_string()); } }
                    _ => {}
                }
            }

            // Flush pending elements on End events
            Event::Empty(e) if in_entry => {
                if e.name().as_ref() == b"re_nokanji" {
                    if let Some(r) = b.current_reading.as_mut() { r.no_kanji = true; }
                }
            }

            Event::Eof => break,
            _ => {}
        }

        // Flush k_ele / r_ele on their end tags
        // (We can't do this inside the End arm without a borrow conflict,
        //  so we check ctx transitions here.)
        if ctx == Ctx::KanjiElement {
            if let Some(k) = b.current_kanji.take() { b.kanji_forms.push(k); }
            ctx = Ctx::Root;
        }
        if ctx == Ctx::ReadingElement {
            if let Some(r) = b.current_reading.take() { b.reading_forms.push(r); }
            ctx = Ctx::Root;
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
    pending_lang: String,
    pending_gtype: Option<String>,
}
