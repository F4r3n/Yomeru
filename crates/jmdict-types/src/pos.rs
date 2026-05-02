use serde::{Deserialize, Serialize};

/// Part-of-speech tags as defined in JMdict.
/// Entity names from the JMdict DTD are mapped to enum variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            "adj-na" => Self::AdjectiveNa,
            "adj-no" => Self::AdjectiveNo,
            "adj-pn" => Self::AdjectivePrenominal,
            "adj-t" => Self::AdjectiveT,
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
            "vs-i" => Self::VerbSuru,
            "vs-s" => Self::VerbSuruS,
            "vs-c" => Self::VerbSuruC,
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
