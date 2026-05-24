use anyhow::{Context, Result};
use kanjidic_types::KanjiEntry;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::path::Path;

pub fn parse_kanjidic(path: &Path) -> Result<Vec<KanjiEntry>> {
    let raw = std::fs::read(path).with_context(|| format!("Failed to open {:?}", path))?;
    parse_kanjidic_bytes(&raw)
}

pub fn parse_kanjidic_bytes(raw: &[u8]) -> Result<Vec<KanjiEntry>> {
    let mut reader = Reader::from_reader(raw);
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
                            b.stroke_count = text.parse().unwrap_or_else(|_| {
                                eprintln!(
                                    "kanjidic-build: invalid stroke_count {:?} for literal {:?}; defaulting to 0",
                                    text, b.literal,
                                );
                                0
                            });
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

#[cfg(test)]
mod tests {
    use super::*;

    const ICHI: &str = r#"<?xml version="1.0"?>
<kanjidic2>
<character>
<literal>一</literal>
<misc>
<grade>1</grade>
<stroke_count>1</stroke_count>
<freq>2</freq>
<jlpt>4</jlpt>
</misc>
<reading_meaning>
<rmgroup>
<reading r_type="ja_on">イチ</reading>
<reading r_type="ja_on">イツ</reading>
<reading r_type="ja_kun">ひと-</reading>
<reading r_type="pinyin">yi1</reading>
<reading r_type="korean_h">일</reading>
<meaning>one</meaning>
<meaning m_lang="fr">un</meaning>
<meaning m_lang="es">uno</meaning>
</rmgroup>
</reading_meaning>
</character>
</kanjidic2>"#;

    #[test]
    fn parses_single_character_with_readings_and_meanings() {
        let entries = parse_kanjidic_bytes(ICHI.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.literal, '一');
        assert_eq!(e.stroke_count, 1);
        assert_eq!(e.grade, Some(1));
        assert_eq!(e.freq, Some(2));
        assert_eq!(e.jlpt, Some(4));
        assert_eq!(e.on_readings, vec!["イチ".to_string(), "イツ".to_string()]);
        assert_eq!(e.kun_readings, vec!["ひと-".to_string()]);
        // Only English meanings (no m_lang attribute) are kept.
        assert_eq!(e.meanings, vec!["one".to_string()]);
    }

    #[test]
    fn skips_non_japanese_readings() {
        // Verified above (pinyin / korean_h dropped), but make it an explicit
        // regression on the r_type filter so a typo doesn't silently let
        // foreign readings into the binary.
        let entries = parse_kanjidic_bytes(ICHI.as_bytes()).unwrap();
        for r in &entries[0].on_readings {
            assert!(r.chars().all(|c| c as u32 >= 0x3041 && c as u32 <= 0x30FF));
        }
    }

    #[test]
    fn parses_multiple_characters() {
        let xml = r#"<kanjidic2>
<character><literal>人</literal>
<misc><stroke_count>2</stroke_count></misc>
<reading_meaning><rmgroup>
<reading r_type="ja_on">ジン</reading>
<meaning>person</meaning>
</rmgroup></reading_meaning>
</character>
<character><literal>口</literal>
<misc><stroke_count>3</stroke_count></misc>
<reading_meaning><rmgroup>
<reading r_type="ja_on">コウ</reading>
<meaning>mouth</meaning>
</rmgroup></reading_meaning>
</character>
</kanjidic2>"#;
        let entries = parse_kanjidic_bytes(xml.as_bytes()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].literal, '人');
        assert_eq!(entries[0].stroke_count, 2);
        assert_eq!(entries[1].literal, '口');
        assert_eq!(entries[1].stroke_count, 3);
    }

    /// Some KANJIDIC entries have variant stroke counts (multiple
    /// `<stroke_count>` elements); the parser must keep only the first.
    #[test]
    fn stroke_count_takes_first_variant() {
        let xml = r#"<kanjidic2>
<character><literal>X</literal>
<misc>
<stroke_count>5</stroke_count>
<stroke_count>6</stroke_count>
</misc>
</character>
</kanjidic2>"#;
        let entries = parse_kanjidic_bytes(xml.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].stroke_count, 5);
    }

    /// Character entries with no `<literal>` body get dropped (build() returns
    /// None when no literal char was accumulated).
    #[test]
    fn drops_character_without_literal() {
        let xml = r#"<kanjidic2>
<character>
<misc><stroke_count>1</stroke_count></misc>
</character>
<character><literal>Y</literal>
<misc><stroke_count>2</stroke_count></misc>
</character>
</kanjidic2>"#;
        let entries = parse_kanjidic_bytes(xml.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].literal, 'Y');
    }
}
