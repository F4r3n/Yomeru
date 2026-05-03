use anyhow::{Context, Result};
use kanjidic_types::KanjiEntry;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::path::Path;

pub fn parse_kanjidic(path: &Path) -> Result<Vec<KanjiEntry>> {
    let raw = std::fs::read(path).with_context(|| format!("Failed to open {:?}", path))?;

    let mut reader = Reader::from_reader(raw.as_slice());
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut buf = Vec::new();
    let mut b = CharBuilder::default();

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Ctx {
        Root,
        Character,
        Literal,
        Misc,
        Grade,
        StrokeCount,
        Freq,
        Jlpt,
        Rmgroup,
        Reading,
        Meaning,
    }

    let mut ctx = Ctx::Root;
    let mut pending_r_type = String::new();
    let mut pending_is_english = false;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => match e.name().as_ref() {
                b"character" => {
                    b = CharBuilder::default();
                    ctx = Ctx::Character;
                }
                b"literal" if ctx == Ctx::Character => ctx = Ctx::Literal,
                b"misc" if ctx == Ctx::Character => ctx = Ctx::Misc,
                b"grade" if ctx == Ctx::Misc => ctx = Ctx::Grade,
                b"stroke_count" if ctx == Ctx::Misc => ctx = Ctx::StrokeCount,
                b"freq" if ctx == Ctx::Misc => ctx = Ctx::Freq,
                b"jlpt" if ctx == Ctx::Misc => ctx = Ctx::Jlpt,
                b"rmgroup" => ctx = Ctx::Rmgroup,
                b"reading" if ctx == Ctx::Rmgroup => {
                    pending_r_type.clear();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"r_type" {
                            pending_r_type =
                                String::from_utf8_lossy(&attr.value).into_owned();
                        }
                    }
                    ctx = Ctx::Reading;
                }
                b"meaning" if ctx == Ctx::Rmgroup => {
                    pending_is_english = true;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"m_lang" {
                            // Non-English language tag — skip this meaning.
                            pending_is_english = false;
                        }
                    }
                    ctx = Ctx::Meaning;
                }
                _ => {}
            },

            Event::End(e) => match e.name().as_ref() {
                b"character" => {
                    if let Some(entry) = b.build() {
                        entries.push(entry);
                    }
                    ctx = Ctx::Root;
                }
                b"literal" => ctx = Ctx::Character,
                b"misc" => ctx = Ctx::Character,
                b"grade" | b"stroke_count" | b"freq" | b"jlpt" => ctx = Ctx::Misc,
                b"rmgroup" => ctx = Ctx::Character,
                b"reading" => ctx = Ctx::Rmgroup,
                b"meaning" => ctx = Ctx::Rmgroup,
                _ => {}
            },

            Event::Text(e) => {
                let text = e.unescape()?.into_owned();
                match ctx {
                    Ctx::Literal => {
                        b.literal = text.chars().next();
                    }
                    Ctx::Grade => {
                        b.grade = text.parse().ok();
                    }
                    Ctx::StrokeCount => {
                        // Only take the first stroke_count (some entries have variants).
                        if b.stroke_count == 0 {
                            b.stroke_count = text.parse().unwrap_or(0);
                        }
                    }
                    Ctx::Freq => {
                        b.freq = text.parse().ok();
                    }
                    Ctx::Jlpt => {
                        b.jlpt = text.parse().ok();
                    }
                    Ctx::Reading => match pending_r_type.as_str() {
                        "ja_on" => b.on_readings.push(text),
                        "ja_kun" => b.kun_readings.push(text),
                        _ => {}
                    },
                    Ctx::Meaning => {
                        if pending_is_english {
                            b.meanings.push(text);
                        }
                    }
                    _ => {}
                }
            }

            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

#[derive(Default)]
struct CharBuilder {
    literal: Option<char>,
    stroke_count: u8,
    grade: Option<u8>,
    freq: Option<u16>,
    jlpt: Option<u8>,
    on_readings: Vec<String>,
    kun_readings: Vec<String>,
    meanings: Vec<String>,
}

impl CharBuilder {
    fn build(&mut self) -> Option<KanjiEntry> {
        Some(KanjiEntry {
            literal: self.literal.take()?,
            stroke_count: self.stroke_count,
            grade: self.grade,
            freq: self.freq,
            jlpt: self.jlpt,
            on_readings: std::mem::take(&mut self.on_readings),
            kun_readings: std::mem::take(&mut self.kun_readings),
            meanings: std::mem::take(&mut self.meanings),
        })
    }
}
