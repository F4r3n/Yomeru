use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::PartOfSpeech;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize_repr,
    Deserialize_repr,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[repr(u8)]
pub enum Field {
    Buddhism = 1,
    Computing = 2,
    FoodCooking = 3,
    Geometry = 4,
    Linguistics = 5,
    MartialArts = 6,
    Mathematics = 7,
    Military = 8,
    Physics = 9,
    Chemistry = 10,
    Architecture = 11,
    Astronomy = 12,
    Baseball = 13,
    Biology = 14,
    Botany = 15,
    Business = 16,
    Economics = 17,
    Engineering = 18,
    Finance = 19,
    Geology = 20,
    Law = 21,
    Medicine = 22,
    Music = 23,
    Shinto = 24,
    Sports = 25,
    Sumo = 26,
    Zoology = 27,
    Anatomy = 28,
    Mahjong = 29,
    Shogi = 30,
    Christianity = 31,
    Philosophy = 32,
    Physiology = 33,
    Pharmacology = 34,
    ElectricityElecEng = 35,
    Entomology = 36,
    Biochemistry = 37,
    Meteorology = 38,
    Trademark = 39,
    Grammar = 40,
    Electronics = 41,
    Psychology = 42,
    Photography = 43,
    GreekMythology = 44,
    Archeology = 45,
    Logic = 46,
    Golf = 47,
    Crystallography = 48,
    Pathology = 49,
    Paleontology = 50,
    Ecology = 51,
    ArtAesthetics = 52,
    Genetics = 53,
    HorseRacing = 54,
    Embryology = 55,
    Geography = 56,
    Fishing = 57,
    GardeningHorticulture = 58,
    Telecommunications = 59,
    MechanicalEngineering = 60,
    Aviation = 61,
    Statistics = 62,
    Agriculture = 63,
    Printing = 64,
    GoGame = 65,
    Hanafuda = 66,
    Audiovisual = 67,
    VideoGames = 68,
    Ornithology = 69,
    Railway = 70,
    Psychiatry = 71,
    Clothing = 72,
    Manga = 73,
    Dentistry = 74,
    CardGames = 75,
    Mining = 76,
    Kabuki = 77,
    Noh = 78,
    Politics = 79,
    StockMarket = 80,
    Skiing = 81,
    RomanMythology = 82,
    Psychoanalysis = 83,
    Film = 84,
    Television = 85,
    ProfessionalWrestling = 86,
    FigureSkating = 87,
    Motorsport = 88,
    CivilEngineering = 89,
    Surgery = 90,
    Mineralogy = 91,
    VeterinaryTerms = 92,
    Boxing = 93,
    Internet = 94,
    ChineseMythology = 95,
    JapaneseMythology = 96,
}

impl Field {
    /// Attempts to parse a JMdict string tag into a Field variant.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "Buddh" => Some(Field::Buddhism),
            "comp" => Some(Field::Computing),
            "food" => Some(Field::FoodCooking),
            "geom" => Some(Field::Geometry),
            "ling" => Some(Field::Linguistics),
            "MA" => Some(Field::MartialArts),
            "math" => Some(Field::Mathematics),
            "mil" => Some(Field::Military),
            "physics" => Some(Field::Physics),
            "chem" => Some(Field::Chemistry),
            "archit" => Some(Field::Architecture),
            "astron" => Some(Field::Astronomy),
            "baseb" => Some(Field::Baseball),
            "biol" => Some(Field::Biology),
            "bot" => Some(Field::Botany),
            "bus" => Some(Field::Business),
            "econ" => Some(Field::Economics),
            "engr" => Some(Field::Engineering),
            "finc" => Some(Field::Finance),
            "geol" => Some(Field::Geology),
            "law" => Some(Field::Law),
            "med" => Some(Field::Medicine),
            "music" => Some(Field::Music),
            "Shinto" => Some(Field::Shinto),
            "sports" => Some(Field::Sports),
            "sumo" => Some(Field::Sumo),
            "zool" => Some(Field::Zoology),
            "anat" => Some(Field::Anatomy),
            "mahj" => Some(Field::Mahjong),
            "shogi" => Some(Field::Shogi),
            "Christn" => Some(Field::Christianity),
            "phil" => Some(Field::Philosophy),
            "physiol" => Some(Field::Physiology),
            "pharm" => Some(Field::Pharmacology),
            "elec" => Some(Field::ElectricityElecEng),
            "ent" => Some(Field::Entomology),
            "biochem" => Some(Field::Biochemistry),
            "met" => Some(Field::Meteorology),
            "tradem" => Some(Field::Trademark),
            "gramm" => Some(Field::Grammar),
            "electr" => Some(Field::Electronics),
            "psych" => Some(Field::Psychology),
            "photo" => Some(Field::Photography),
            "grmyth" => Some(Field::GreekMythology),
            "archeol" => Some(Field::Archeology),
            "logic" => Some(Field::Logic),
            "golf" => Some(Field::Golf),
            "cryst" => Some(Field::Crystallography),
            "pathol" => Some(Field::Pathology),
            "paleo" => Some(Field::Paleontology),
            "ecol" => Some(Field::Ecology),
            "art" => Some(Field::ArtAesthetics),
            "genet" => Some(Field::Genetics),
            "horse" => Some(Field::HorseRacing),
            "embryo" => Some(Field::Embryology),
            "geogr" => Some(Field::Geography),
            "fish" => Some(Field::Fishing),
            "gardn" => Some(Field::GardeningHorticulture),
            "telec" => Some(Field::Telecommunications),
            "mech" => Some(Field::MechanicalEngineering),
            "aviat" => Some(Field::Aviation),
            "stat" => Some(Field::Statistics),
            "agric" => Some(Field::Agriculture),
            "print" => Some(Field::Printing),
            "go" => Some(Field::GoGame),
            "hanaf" => Some(Field::Hanafuda),
            "audvid" => Some(Field::Audiovisual),
            "vidg" => Some(Field::VideoGames),
            "ornith" => Some(Field::Ornithology),
            "rail" => Some(Field::Railway),
            "psy" => Some(Field::Psychiatry),
            "cloth" => Some(Field::Clothing),
            "manga" => Some(Field::Manga),
            "dent" => Some(Field::Dentistry),
            "cards" => Some(Field::CardGames),
            "mining" => Some(Field::Mining),
            "kabuki" => Some(Field::Kabuki),
            "noh" => Some(Field::Noh),
            "politics" => Some(Field::Politics),
            "stockm" => Some(Field::StockMarket),
            "ski" => Some(Field::Skiing),
            "rommyth" => Some(Field::RomanMythology),
            "psyanal" => Some(Field::Psychoanalysis),
            "film" => Some(Field::Film),
            "tv" => Some(Field::Television),
            "prowres" => Some(Field::ProfessionalWrestling),
            "figskt" => Some(Field::FigureSkating),
            "motor" => Some(Field::Motorsport),
            "civeng" => Some(Field::CivilEngineering),
            "surg" => Some(Field::Surgery),
            "min" => Some(Field::Mineralogy),
            "vet" => Some(Field::VeterinaryTerms),
            "boxing" => Some(Field::Boxing),
            "internet" => Some(Field::Internet),
            "chmyth" => Some(Field::ChineseMythology),
            "jpmyth" => Some(Field::JapaneseMythology),
            _ => None,
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize_repr,
    Deserialize_repr,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[repr(u8)]
pub enum Misc {
    Abbreviation = 1,
    Aphorism = 2,
    Archaic = 3,
    Character = 4,
    ChildrensLanguage = 5,
    Colloquial = 6,
    CompanyName = 7,
    Creature = 8,
    DatedTerm = 9,
    Deity = 10,
    Derogatory = 11,
    Document = 12,
    Euphemistic = 13,
    Event = 14,
    FamiliarLanguage = 15,
    FemaleTermOrLanguage = 16,
    Fiction = 17,
    FormOfWord = 18,
    GivenName = 19,
    Group = 20,
    HistoricalTerm = 21,
    HonorificRespectfulLanguage = 22,
    HumbleLanguage = 23,
    IdiomaticExpression = 24,
    JocularHumorousTerm = 25,
    LegalTerm = 26,
    MangaSlang = 27,
    MaleTermOrLanguage = 28,
    Mythology = 29,
    InternetSlang = 30,
    Object = 31,
    ObsoleteTerm = 32,
    OnomatopoeicMimeticWord = 33,
    OrganizationName = 34,
    Other = 35,
    PersonName = 36,
    PlaceName = 37,
    PoeticTerm = 38,
    PoliteLanguage = 39,
    ProductName = 40,
    Proverb = 41,
    Quotation = 42,
    RareTerm = 43,
    ReligiousTerm = 44,
    SensitiveTerm = 45,
    Service = 46,
    ShipName = 47,
    Slang = 48,
    RailwayStationName = 49,
    Surname = 50,
    UsuallyKana = 51,
    UnclassifiedName = 52,
    VulgarExpression = 53,
    WorkName = 54,
    RudeOrXRatedTerm = 55,
    Yojijukugo = 56,
}

impl Misc {
    /// Attempts to parse a JMdict string tag into a Misc variant.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "abbr" => Some(Misc::Abbreviation),
            "aph" => Some(Misc::Aphorism),
            "arch" => Some(Misc::Archaic),
            "char" => Some(Misc::Character),
            "chn" => Some(Misc::ChildrensLanguage),
            "col" => Some(Misc::Colloquial),
            "company" => Some(Misc::CompanyName),
            "creat" => Some(Misc::Creature),
            "dated" => Some(Misc::DatedTerm),
            "dei" => Some(Misc::Deity),
            "derog" => Some(Misc::Derogatory),
            "doc" => Some(Misc::Document),
            "euph" => Some(Misc::Euphemistic),
            "ev" => Some(Misc::Event),
            "fam" => Some(Misc::FamiliarLanguage),
            "fem" => Some(Misc::FemaleTermOrLanguage),
            "fict" => Some(Misc::Fiction),
            "form" => Some(Misc::FormOfWord),
            "given" => Some(Misc::GivenName),
            "group" => Some(Misc::Group),
            "hist" => Some(Misc::HistoricalTerm),
            "hon" => Some(Misc::HonorificRespectfulLanguage),
            "hum" => Some(Misc::HumbleLanguage),
            "id" => Some(Misc::IdiomaticExpression),
            "joc" => Some(Misc::JocularHumorousTerm),
            "leg" => Some(Misc::LegalTerm),
            "m-sl" => Some(Misc::MangaSlang),
            "male" => Some(Misc::MaleTermOrLanguage),
            "myth" => Some(Misc::Mythology),
            "net-sl" => Some(Misc::InternetSlang),
            "obj" => Some(Misc::Object),
            "obs" => Some(Misc::ObsoleteTerm),
            "on-mim" => Some(Misc::OnomatopoeicMimeticWord),
            "org" => Some(Misc::OrganizationName),
            "oth" => Some(Misc::Other),
            "person" => Some(Misc::PersonName),
            "place" => Some(Misc::PlaceName),
            "poet" => Some(Misc::PoeticTerm),
            "pol" => Some(Misc::PoliteLanguage),
            "product" => Some(Misc::ProductName),
            "proverb" => Some(Misc::Proverb),
            "quote" => Some(Misc::Quotation),
            "rare" => Some(Misc::RareTerm),
            "relig" => Some(Misc::ReligiousTerm),
            "sens" => Some(Misc::SensitiveTerm),
            "serv" => Some(Misc::Service),
            "ship" => Some(Misc::ShipName),
            "sl" => Some(Misc::Slang),
            "station" => Some(Misc::RailwayStationName),
            "surname" => Some(Misc::Surname),
            "uk" => Some(Misc::UsuallyKana),
            "unclass" => Some(Misc::UnclassifiedName),
            "vulg" => Some(Misc::VulgarExpression),
            "work" => Some(Misc::WorkName),
            "X" => Some(Misc::RudeOrXRatedTerm),
            "yoji" => Some(Misc::Yojijukugo),
            _ => None,
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize_repr,
    Deserialize_repr,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[repr(u8)]
pub enum KanjiInf {
    IrregularKanji = 1,
    IrregularOkurigana = 2,
    OutdatedKanji = 3,
    IrregularKana = 4,
    Ateji = 5,
    RareKanji = 6,
    SearchOnlyKanji = 7,
}

impl KanjiInf {
    /// Attempts to parse a JMdict string tag into an Inf variant.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "iK" => Some(KanjiInf::IrregularKanji),
            "io" => Some(KanjiInf::IrregularOkurigana),
            "oK" => Some(KanjiInf::OutdatedKanji),
            "ik" => Some(KanjiInf::IrregularKana),
            "ateji" => Some(KanjiInf::Ateji),
            "rK" => Some(KanjiInf::RareKanji),
            "sK" => Some(KanjiInf::SearchOnlyKanji),
            _ => None,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize_repr,
    Deserialize_repr,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[repr(u8)]
pub enum FreqKind {
    Gai = 1,
    Ichi = 2,
    News = 3,
    Nf = 4,
    Spec = 5,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
pub struct Freq {
    pub kind: FreqKind,
    pub value: u8,
}

impl Freq {
    /// Attempts to parse a JMdict frequency tag (e.g., "news1", "nf24") into a Freq struct.
    pub fn from_tag(tag: &str) -> Option<Self> {
        // Find where the alphabetical prefix ends and the numeric suffix begins
        let split_idx = tag.find(|c: char| c.is_ascii_digit())?;
        let (prefix, suffix) = tag.split_at(split_idx);

        let value: u8 = suffix.parse().ok()?;

        let kind = match prefix {
            "gai" if (1..=2).contains(&value) => FreqKind::Gai,
            "ichi" if (1..=2).contains(&value) => FreqKind::Ichi,
            "news" if (1..=2).contains(&value) => FreqKind::News,
            "nf" if (1..=48).contains(&value) => FreqKind::Nf,
            "spec" if (1..=2).contains(&value) => FreqKind::Spec,
            _ => return None,
        };

        Some(Freq { kind, value })
    }
}

/// A single JMdict dictionary entry.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
pub struct WordEntry {
    /// Sequence number from JMdict (unique entry ID).
    pub sequence: u32,
    /// Kanji (non-kana) writing forms, e.g. ["飲む", "飮む"].
    pub kanji_forms: Vec<KanjiElement>,
    /// Reading (kana) forms, e.g. ["のむ"].
    pub reading_forms: Vec<ReadingElement>,
    /// Senses (meanings), each sense can have multiple glosses and POS tags.
    pub senses: Vec<Sense>,
}

impl WordEntry {
    /// Primary headword: first kanji form, or first reading if no kanji.
    pub fn headword(&self) -> &str {
        self.kanji_forms
            .first()
            .map(|k| k.text.as_str())
            .or_else(|| self.reading_forms.first().map(|r| r.text.as_str()))
            .unwrap_or("")
    }

    /// Primary reading (first reading form).
    pub fn primary_reading(&self) -> &str {
        self.reading_forms
            .first()
            .map(|r| r.text.as_str())
            .unwrap_or("")
    }

    /// First English gloss from the first sense.
    pub fn first_gloss(&self) -> &str {
        self.senses
            .first()
            .and_then(|s| s.glosses.first())
            .map(|g| g.text.as_str())
            .unwrap_or("")
    }
}

impl ArchivedWordEntry {
    /// Primary headword: first kanji form, or first reading if no kanji.
    /// Mirror of [`WordEntry::headword`] over the zero-copy archived layout.
    pub fn headword(&self) -> &str {
        self.kanji_forms
            .first()
            .map(|k| k.text.as_str())
            .or_else(|| self.reading_forms.first().map(|r| r.text.as_str()))
            .unwrap_or("")
    }

    /// Primary reading (first reading form).
    pub fn primary_reading(&self) -> &str {
        self.reading_forms
            .first()
            .map(|r| r.text.as_str())
            .unwrap_or("")
    }

    /// First English gloss from the first sense.
    pub fn first_gloss(&self) -> &str {
        self.senses
            .first()
            .and_then(|s| s.glosses.first())
            .map(|g| g.text.as_str())
            .unwrap_or("")
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
pub struct KanjiElement {
    pub text: String,
    /// `ke_inf` tags — e.g. "rK" (rarely used kanji form), "sK" (search-only),
    /// "iK" (irregular), "oK" (out-dated).
    pub info: Vec<KanjiInf>,
    /// `ke_pri` frequency tags — e.g. "news1", "ichi1", "spec1", "gai1",
    /// "nf01"–"nf48".
    pub priorities: Vec<Freq>,
}

impl KanjiElement {
    pub fn from_text(content: impl Into<String>) -> Self {
        Self {
            text: content.into(),
            info: vec![],
            priorities: vec![],
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
pub struct ReadingElement {
    pub text: String,
    #[cfg(feature = "full")]
    /// If true, this reading only applies to certain kanji forms.
    pub no_kanji: bool,
    #[cfg(feature = "full")]
    /// Kanji forms this reading applies to (empty = all).
    pub restricted_to: Vec<String>,
    #[cfg(feature = "full")]
    pub info: Vec<String>,
    /// `re_pri` frequency tags — same vocabulary as `KanjiElement.priorities`.
    pub priorities: Vec<Freq>,
}

impl ReadingElement {
    pub fn from_reading(reading: impl Into<String>) -> Self {
        Self {
            text: reading.into(),
            #[cfg(feature = "full")]
            no_kanji: false,
            #[cfg(feature = "full")]
            restricted_to: vec![],
            #[cfg(feature = "full")]
            info: vec![],
            priorities: vec![],
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Default,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
pub struct Sense {
    /// Part-of-speech tags (carry forward from previous sense if empty).
    pub pos: Vec<PartOfSpeech>,
    /// English glosses (translations).
    pub glosses: Vec<Gloss>,
    #[cfg(feature = "full")]
    /// Cross-references to other entries.
    pub xrefs: Vec<String>,
    #[cfg(feature = "full")]
    /// Antonyms.
    pub antonyms: Vec<String>,
    #[cfg(feature = "full")]
    /// Field of application (e.g. "math", "food").
    pub fields: Vec<Field>,
    /// Miscellaneous tags — most usefully "uk" (usually written in kana).
    pub misc: Vec<Misc>,
    #[cfg(feature = "full")]
    /// Sense-level info notes.
    pub info: Vec<String>,
    #[cfg(feature = "full")]
    /// Dialect tags.
    pub dialects: Vec<String>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
pub struct Gloss {
    pub text: String,
    /// Gloss type ("lit", "fig", "expl", etc.).
    pub gloss_type: Option<String>,
}

impl Gloss {
    pub fn new(content: impl Into<String>, gloss_type: Option<String>) -> Self {
        Self {
            text: content.into(),
            gloss_type,
        }
    }
}
