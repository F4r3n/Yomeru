//! Hand-written `serde::Serialize` for the rkyv **archived** entry types.
//!
//! This is what lets the server / wasm boundary serialize lookup results
//! *directly from the zero-copy archived buffer* to JSON / JsValue, without
//! ever materializing an owned [`WordEntry`]. Each impl reproduces the **exact**
//! JSON shape that `#[derive(Serialize)]` produces for the owned type, so
//! clients see byte-identical output. The `tests::wire_compat` round-trip below
//! (and in `jmdict-core`) guards that equivalence.
//!
//! Strings are emitted via `ArchivedString::as_str` (borrowed, no copy); nested
//! archived structs serialize themselves, so `&[Archived…]` slices are
//! `Serialize` automatically. Only `Vec<String>` and `Option<String>` need a
//! thin adapter. The leaf [`PartOfSpeech`] enum is cheap to rkyv-deserialize
//! (fieldless, no heap), so we round-trip it to the owned type and let serde's
//! derive emit the kebab-case name — guaranteeing the rename matches.

#[cfg(feature = "full")]
use serde::ser::SerializeSeq;
use serde::ser::{Serialize, SerializeStruct, Serializer};

#[cfg(feature = "full")]
use crate::entry::{ArchivedField, Field};
use crate::entry::{ArchivedFreq, ArchivedFreqKind, FreqKind};
use crate::entry::{
    ArchivedGloss, ArchivedKanjiElement, ArchivedReadingElement, ArchivedSense, ArchivedWordEntry,
};
use crate::entry::{ArchivedKanjiInf, KanjiInf};
use crate::entry::{ArchivedMisc, Misc};

use crate::pos::{ArchivedPartOfSpeech, PartOfSpeech};

#[cfg(feature = "full")]
use rkyv::string::ArchivedString;
#[cfg(feature = "full")]
use rkyv::vec::ArchivedVec;

/// Serializes an archived `Vec<String>` as a JSON array of strings.
#[cfg(feature = "full")]
struct StrSeq<'a>(&'a ArchivedVec<ArchivedString>);

#[cfg(feature = "full")]
impl Serialize for StrSeq<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for item in self.0.iter() {
            seq.serialize_element(item.as_str())?;
        }
        seq.end()
    }
}

impl Serialize for ArchivedPartOfSpeech {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Fieldless enum → rkyv-deserialize is alloc-free; reuse the owned
        // serde derive so the kebab-case rename can never drift.
        let owned: PartOfSpeech = rkyv::deserialize::<PartOfSpeech, rkyv::rancor::Error>(self)
            .map_err(serde::ser::Error::custom)?;
        owned.serialize(s)
    }
}

#[cfg(feature = "full")]
impl Serialize for ArchivedField {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Fieldless enum → rkyv-deserialize is alloc-free; reuse the owned
        // serde derive so the variant naming can never drift.
        let owned: Field = rkyv::deserialize::<Field, rkyv::rancor::Error>(self)
            .map_err(serde::ser::Error::custom)?;
        owned.serialize(s)
    }
}

impl Serialize for ArchivedMisc {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let owned: Misc = rkyv::deserialize::<Misc, rkyv::rancor::Error>(self)
            .map_err(serde::ser::Error::custom)?;
        owned.serialize(s)
    }
}

impl Serialize for ArchivedKanjiInf {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let owned: KanjiInf = rkyv::deserialize::<KanjiInf, rkyv::rancor::Error>(self)
            .map_err(serde::ser::Error::custom)?;
        owned.serialize(s)
    }
}

impl Serialize for ArchivedGloss {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Gloss", 2)?;
        st.serialize_field("text", self.text.as_str())?;
        st.serialize_field("gloss_type", &self.gloss_type.as_ref().map(|g| g.as_str()))?;
        st.end()
    }
}

impl Serialize for ArchivedFreqKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let owned: FreqKind = rkyv::deserialize::<FreqKind, rkyv::rancor::Error>(self)
            .map_err(serde::ser::Error::custom)?;
        owned.serialize(s)
    }
}

impl Serialize for ArchivedFreq {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Freq", 2)?;
        st.serialize_field("kind", &self.kind)?;
        st.serialize_field("value", &self.value)?;
        st.end()
    }
}

impl Serialize for ArchivedKanjiElement {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("KanjiElement", 3)?;
        st.serialize_field("text", self.text.as_str())?;
        st.serialize_field("info", self.info.as_slice())?;
        st.serialize_field("priorities", self.priorities.as_slice())?;
        st.end()
    }
}

impl Serialize for ArchivedReadingElement {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let n = if cfg!(feature = "full") { 5 } else { 2 };
        let mut st = s.serialize_struct("ReadingElement", n)?;
        st.serialize_field("text", self.text.as_str())?;
        #[cfg(feature = "full")]
        {
            st.serialize_field("no_kanji", &self.no_kanji)?;
            st.serialize_field("restricted_to", &StrSeq(&self.restricted_to))?;
            st.serialize_field("info", &StrSeq(&self.info))?;
        }
        st.serialize_field("priorities", self.priorities.as_slice())?;
        st.end()
    }
}

impl Serialize for ArchivedSense {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let n = if cfg!(feature = "full") { 8 } else { 3 };
        let mut st = s.serialize_struct("Sense", n)?;
        st.serialize_field("pos", self.pos.as_slice())?;
        st.serialize_field("glosses", self.glosses.as_slice())?;
        #[cfg(feature = "full")]
        {
            st.serialize_field("xrefs", &StrSeq(&self.xrefs))?;
            st.serialize_field("antonyms", &StrSeq(&self.antonyms))?;
            st.serialize_field("fields", self.fields.as_slice())?;
        }
        st.serialize_field("misc", self.misc.as_slice())?;
        #[cfg(feature = "full")]
        {
            st.serialize_field("info", &StrSeq(&self.info))?;
            st.serialize_field("dialects", &StrSeq(&self.dialects))?;
        }
        st.end()
    }
}

impl Serialize for ArchivedWordEntry {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("WordEntry", 4)?;
        st.serialize_field("sequence", &self.sequence.to_native())?;
        st.serialize_field("kanji_forms", self.kanji_forms.as_slice())?;
        st.serialize_field("reading_forms", self.reading_forms.as_slice())?;
        st.serialize_field("senses", self.senses.as_slice())?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::entry::{ArchivedWordEntry, Gloss, KanjiElement, ReadingElement, Sense, WordEntry};
    use crate::pos::PartOfSpeech;
    use crate::{Freq, KanjiInf, Misc};

    fn sample() -> WordEntry {
        WordEntry {
            sequence: 1234567,
            kanji_forms: vec![KanjiElement {
                text: "食べる".into(),
                info: vec![KanjiInf::RareKanji],
                priorities: vec![
                    Freq {
                        kind: crate::FreqKind::News,
                        value: 1,
                    },
                    Freq {
                        kind: crate::FreqKind::Nf,
                        value: 12,
                    },
                ],
            }],
            reading_forms: vec![
                ReadingElement::from_reading("たべる"),
                ReadingElement::from_reading("くう"),
            ],
            senses: vec![
                Sense {
                    pos: vec![PartOfSpeech::VerbIchidan, PartOfSpeech::VerbTransitive],
                    glosses: vec![
                        Gloss::new("to eat", None),
                        Gloss::new("to live on", Some("fig".into())),
                    ],
                    misc: vec![Misc::UsuallyKana],
                    ..Default::default()
                },
                Sense::default(),
            ],
        }
    }

    /// The hand-written archived `Serialize` must produce byte-identical JSON to
    /// the owned serde derive — this is what guarantees clients are unaffected.
    #[test]
    fn wire_compat_archived_matches_owned() {
        let owned = sample();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&owned).unwrap();
        let archived = rkyv::access::<ArchivedWordEntry, rkyv::rancor::Error>(&bytes).unwrap();

        let owned_json = serde_json::to_value(&owned).unwrap();
        let archived_json = serde_json::to_value(archived).unwrap();
        assert_eq!(owned_json, archived_json);
    }
}
