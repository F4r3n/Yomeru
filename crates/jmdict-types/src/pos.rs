use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Part-of-speech tags as defined in JMdict.
/// Entity names from the JMdict DTD are mapped to enum variants.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum PartOfSpeech {
    // Adjectives
    Adjective,          // adj-i
    AdjectiveNa,        // adj-na
    AdjectiveNo,        // adj-no
    AdjectivePrenominal, // adj-pn
    AdjectiveT,         // adj-t (taru)
    AdjectiveFormal,    // adj-f (TODO: not in old JMdict, kept for compat)

    // Adverb
    Adverb,             // adv
    AdverbTo,           // adv-to

    // Auxiliary
    Auxiliary,          // aux
    AuxiliaryAdjective, // aux-adj
    AuxiliaryVerb,      // aux-v

    // Conjunction / interjection / particle / prefix / suffix
    Conjunction,        // conj
    Interjection,       // int
    Particle,           // prt
    Prefix,             // pref
    Suffix,             // suf

    // Nouns
    Noun,               // n
    NounAdverbial,      // n-adv
    NounProper,         // n-pr  (proper noun)
    NounSuffix,         // n-suf
    NounPrefix,         // n-pref
    NounTemporal,       // n-t

    // Numeric / counter
    Numeric,            // num
    Counter,            // ctr

    // Verbs — ichidan
    VerbIchidan,        // v1
    VerbIchidanS,       // v1-s (kureru)

    // Verbs — godan
    VerbGodanBu,        // v5b
    VerbGodanGu,        // v5g
    VerbGodanKu,        // v5k
    VerbGodanKuS,       // v5k-s
    VerbGodanMu,        // v5m
    VerbGodanNu,        // v5n
    VerbGodanRu,        // v5r
    VerbGodanRuIrr,     // v5r-i
    VerbGodanSu,        // v5s
    VerbGodanTsu,       // v5t
    VerbGodanU,         // v5u
    VerbGodanUS,        // v5u-s
    VerbGodanUru,       // v5uru

    // Verbs — irregular / special
    VerbSuru,           // vs-i (suru — included form)
    VerbSuruS,          // vs-s (suru — special class)
    VerbSuruC,          // vs-c (su — classical)
    VerbKuru,           // vk
    VerbNu,             // vn (nu)
    VerbRu,             // vr (ru — irregular)
    VerbUnclassified,   // v-unspec
    VerbTransitive,     // vt
    VerbIntransitive,   // vi

    // Expression / copula / pronoun
    Expression,         // exp
    Copula,             // cop
    Pronoun,            // pn

    // Unknown/other
    Unknown,
}

impl PartOfSpeech {
    /// Parse from the JMdict entity string (e.g. "&v5b;" → VerbGodanBu).
    pub fn from_entity(entity: &str) -> Self {
        match entity {
            "adj-i" => Self::Adjective,
            "adj-ix" => Self::Adjective,
            "adj-na" => Self::AdjectiveNa,
            "adj-no" => Self::AdjectiveNo,
            "adj-pn" => Self::AdjectivePrenominal,
            "adj-t" => Self::AdjectiveT,
            "adj-f" => Self::AdjectiveFormal,
            "adv" => Self::Adverb,
            "adv-to" => Self::AdverbTo,
            "aux" => Self::Auxiliary,
            "aux-adj" => Self::AuxiliaryAdjective,
            "aux-v" => Self::AuxiliaryVerb,
            "conj" => Self::Conjunction,
            "int" => Self::Interjection,
            "prt" => Self::Particle,
            "pref" => Self::Prefix,
            "suf" => Self::Suffix,
            "n" => Self::Noun,
            "n-adv" => Self::NounAdverbial,
            "n-pr" => Self::NounProper,
            "n-suf" => Self::NounSuffix,
            "n-pref" => Self::NounPrefix,
            "n-t" => Self::NounTemporal,
            "num" => Self::Numeric,
            "ctr" => Self::Counter,
            "v1" => Self::VerbIchidan,
            "v1-s" => Self::VerbIchidanS,
            "v5b" => Self::VerbGodanBu,
            "v5g" => Self::VerbGodanGu,
            "v5k" => Self::VerbGodanKu,
            "v5k-s" => Self::VerbGodanKuS,
            "v5m" => Self::VerbGodanMu,
            "v5n" => Self::VerbGodanNu,
            "v5r" => Self::VerbGodanRu,
            "v5r-i" => Self::VerbGodanRuIrr,
            "v5s" => Self::VerbGodanSu,
            "v5t" => Self::VerbGodanTsu,
            "v5u" => Self::VerbGodanU,
            "v5u-s" => Self::VerbGodanUS,
            "v5uru" => Self::VerbGodanUru,
            "vs" => Self::VerbSuru,
            "vs-i" => Self::VerbSuru,
            "vs-s" => Self::VerbSuruS,
            "vs-c" => Self::VerbSuruC,
            "vz" => Self::VerbIchidan,
            "vk" => Self::VerbKuru,
            "vn" => Self::VerbNu,
            "vr" => Self::VerbRu,
            "v-unspec" => Self::VerbUnclassified,
            "vt" => Self::VerbTransitive,
            "vi" => Self::VerbIntransitive,
            "exp" => Self::Expression,
            "cop" => Self::Copula,
            "pn" => Self::Pronoun,
            _ => Self::Unknown,
        }
    }

    pub fn display_short(&self) -> &'static str {
        match self {
            Self::Adjective => "adj-i",
            Self::AdjectiveNa => "adj-na",
            Self::AdjectiveNo => "adj-no",
            Self::Noun => "n",
            Self::VerbIchidan => "v1",
            Self::VerbGodanBu | Self::VerbGodanGu | Self::VerbGodanKu
            | Self::VerbGodanMu | Self::VerbGodanNu | Self::VerbGodanRu
            | Self::VerbGodanSu | Self::VerbGodanTsu | Self::VerbGodanU => "v5",
            Self::VerbSuru | Self::VerbSuruS | Self::VerbSuruC => "vs",
            Self::VerbKuru => "vk",
            Self::Adverb | Self::AdverbTo => "adv",
            Self::Particle => "prt",
            Self::Expression => "exp",
            _ => "?",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_entity_adjectives() {
        assert_eq!(PartOfSpeech::from_entity("adj-i"), PartOfSpeech::Adjective);
        assert_eq!(PartOfSpeech::from_entity("adj-na"), PartOfSpeech::AdjectiveNa);
        assert_eq!(PartOfSpeech::from_entity("adj-no"), PartOfSpeech::AdjectiveNo);
        assert_eq!(PartOfSpeech::from_entity("adj-pn"), PartOfSpeech::AdjectivePrenominal);
        assert_eq!(PartOfSpeech::from_entity("adj-t"), PartOfSpeech::AdjectiveT);
    }

    #[test]
    fn from_entity_adverbs_and_aux() {
        assert_eq!(PartOfSpeech::from_entity("adv"), PartOfSpeech::Adverb);
        assert_eq!(PartOfSpeech::from_entity("adv-to"), PartOfSpeech::AdverbTo);
        assert_eq!(PartOfSpeech::from_entity("aux"), PartOfSpeech::Auxiliary);
        assert_eq!(PartOfSpeech::from_entity("aux-adj"), PartOfSpeech::AuxiliaryAdjective);
        assert_eq!(PartOfSpeech::from_entity("aux-v"), PartOfSpeech::AuxiliaryVerb);
    }

    #[test]
    fn from_entity_nouns() {
        assert_eq!(PartOfSpeech::from_entity("n"), PartOfSpeech::Noun);
        assert_eq!(PartOfSpeech::from_entity("n-adv"), PartOfSpeech::NounAdverbial);
        assert_eq!(PartOfSpeech::from_entity("n-pr"), PartOfSpeech::NounProper);
        assert_eq!(PartOfSpeech::from_entity("n-suf"), PartOfSpeech::NounSuffix);
        assert_eq!(PartOfSpeech::from_entity("n-pref"), PartOfSpeech::NounPrefix);
        assert_eq!(PartOfSpeech::from_entity("n-t"), PartOfSpeech::NounTemporal);
    }

    #[test]
    fn from_entity_ichidan_verbs() {
        assert_eq!(PartOfSpeech::from_entity("v1"), PartOfSpeech::VerbIchidan);
        assert_eq!(PartOfSpeech::from_entity("v1-s"), PartOfSpeech::VerbIchidanS);
    }

    #[test]
    fn from_entity_godan_verbs() {
        assert_eq!(PartOfSpeech::from_entity("v5b"), PartOfSpeech::VerbGodanBu);
        assert_eq!(PartOfSpeech::from_entity("v5g"), PartOfSpeech::VerbGodanGu);
        assert_eq!(PartOfSpeech::from_entity("v5k"), PartOfSpeech::VerbGodanKu);
        assert_eq!(PartOfSpeech::from_entity("v5k-s"), PartOfSpeech::VerbGodanKuS);
        assert_eq!(PartOfSpeech::from_entity("v5m"), PartOfSpeech::VerbGodanMu);
        assert_eq!(PartOfSpeech::from_entity("v5n"), PartOfSpeech::VerbGodanNu);
        assert_eq!(PartOfSpeech::from_entity("v5r"), PartOfSpeech::VerbGodanRu);
        assert_eq!(PartOfSpeech::from_entity("v5r-i"), PartOfSpeech::VerbGodanRuIrr);
        assert_eq!(PartOfSpeech::from_entity("v5s"), PartOfSpeech::VerbGodanSu);
        assert_eq!(PartOfSpeech::from_entity("v5t"), PartOfSpeech::VerbGodanTsu);
        assert_eq!(PartOfSpeech::from_entity("v5u"), PartOfSpeech::VerbGodanU);
        assert_eq!(PartOfSpeech::from_entity("v5u-s"), PartOfSpeech::VerbGodanUS);
        assert_eq!(PartOfSpeech::from_entity("v5uru"), PartOfSpeech::VerbGodanUru);
    }

    #[test]
    fn from_entity_irregular_verbs() {
        assert_eq!(PartOfSpeech::from_entity("vs-i"), PartOfSpeech::VerbSuru);
        assert_eq!(PartOfSpeech::from_entity("vs-s"), PartOfSpeech::VerbSuruS);
        assert_eq!(PartOfSpeech::from_entity("vs-c"), PartOfSpeech::VerbSuruC);
        assert_eq!(PartOfSpeech::from_entity("vk"), PartOfSpeech::VerbKuru);
        assert_eq!(PartOfSpeech::from_entity("vn"), PartOfSpeech::VerbNu);
        assert_eq!(PartOfSpeech::from_entity("vr"), PartOfSpeech::VerbRu);
        assert_eq!(PartOfSpeech::from_entity("v-unspec"), PartOfSpeech::VerbUnclassified);
        assert_eq!(PartOfSpeech::from_entity("vt"), PartOfSpeech::VerbTransitive);
        assert_eq!(PartOfSpeech::from_entity("vi"), PartOfSpeech::VerbIntransitive);
    }

    #[test]
    fn from_entity_misc() {
        assert_eq!(PartOfSpeech::from_entity("conj"), PartOfSpeech::Conjunction);
        assert_eq!(PartOfSpeech::from_entity("int"), PartOfSpeech::Interjection);
        assert_eq!(PartOfSpeech::from_entity("prt"), PartOfSpeech::Particle);
        assert_eq!(PartOfSpeech::from_entity("pref"), PartOfSpeech::Prefix);
        assert_eq!(PartOfSpeech::from_entity("suf"), PartOfSpeech::Suffix);
        assert_eq!(PartOfSpeech::from_entity("num"), PartOfSpeech::Numeric);
        assert_eq!(PartOfSpeech::from_entity("ctr"), PartOfSpeech::Counter);
        assert_eq!(PartOfSpeech::from_entity("exp"), PartOfSpeech::Expression);
        assert_eq!(PartOfSpeech::from_entity("cop"), PartOfSpeech::Copula);
        assert_eq!(PartOfSpeech::from_entity("pn"), PartOfSpeech::Pronoun);
    }

    #[test]
    fn from_entity_unknown_fallback() {
        assert_eq!(PartOfSpeech::from_entity(""), PartOfSpeech::Unknown);
        assert_eq!(PartOfSpeech::from_entity("xyz"), PartOfSpeech::Unknown);
        assert_eq!(PartOfSpeech::from_entity("v5"), PartOfSpeech::Unknown);
    }

    #[test]
    fn display_short_adjectives() {
        assert_eq!(PartOfSpeech::Adjective.display_short(), "adj-i");
        assert_eq!(PartOfSpeech::AdjectiveNa.display_short(), "adj-na");
        assert_eq!(PartOfSpeech::AdjectiveNo.display_short(), "adj-no");
    }

    #[test]
    fn display_short_verbs() {
        assert_eq!(PartOfSpeech::VerbIchidan.display_short(), "v1");
        assert_eq!(PartOfSpeech::VerbGodanBu.display_short(), "v5");
        assert_eq!(PartOfSpeech::VerbGodanRu.display_short(), "v5");
        assert_eq!(PartOfSpeech::VerbSuru.display_short(), "vs");
        assert_eq!(PartOfSpeech::VerbSuruC.display_short(), "vs");
        assert_eq!(PartOfSpeech::VerbKuru.display_short(), "vk");
    }

    #[test]
    fn display_short_fallback() {
        assert_eq!(PartOfSpeech::Unknown.display_short(), "?");
        assert_eq!(PartOfSpeech::Copula.display_short(), "?");
        assert_eq!(PartOfSpeech::VerbGodanRuIrr.display_short(), "?");
    }
}
