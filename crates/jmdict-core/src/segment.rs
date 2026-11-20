//! The words of a sentence, as the cheapest path through every word that
//! could start at each char: a lattice, as MeCab builds, with costs set by
//! hand from what JMdict knows (part of speech, frequency tags, kana or
//! kanji) instead of trained on a corpus.
//!
//! Taking the longest word at each point reads がある as があ|る and はい
//! where は|い was meant; weighing whole paths prefers two common words over
//! a rare one and a leftover kana. `tests/segmentation.rs` scores it against
//! the Tanaka corpus.

use deinflect::{Reason, Reasons};
use jmdict_types::{ArchivedFreqKind, ArchivedMisc, ArchivedPartOfSpeech as P, ArchivedWordEntry};

use crate::dictionary::{FstWalk, get_entry, get_entry_group};

/// The longest word tried, in chars.
const MAX_WORD_CHARS: usize = 20;

/// What a word is to its neighbours: the part of speech, coarsely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Particle,
    Noun,
    NaAdjective,
    Verb,
    Adjective,
    /// Auxiliaries and the copula: だ, です, らしい, そうだ.
    Auxiliary,
    Adverb,
    Prefix,
    /// Suffixes and counters: さん, 的, 個.
    Suffix,
    /// この, その, 大きな.
    Prenominal,
    Conjunction,
    Interjection,
    Expression,
    /// No dictionary word: a name in katakana, a rare kanji.
    Unknown,
}

const CLASSES: [Class; 14] = [
    Class::Particle,
    Class::Noun,
    Class::NaAdjective,
    Class::Verb,
    Class::Adjective,
    Class::Auxiliary,
    Class::Adverb,
    Class::Prefix,
    Class::Suffix,
    Class::Prenominal,
    Class::Conjunction,
    Class::Interjection,
    Class::Expression,
    Class::Unknown,
];

/// A lattice state: the class of the word just read, or the start of a
/// clause (the sentence's start, or past punctuation).
const START: usize = CLASSES.len();
/// Past a number written in digits (3月): counters follow as after a noun.
const AFTER_NUMBER: usize = CLASSES.len() + 1;
const STATES: usize = CLASSES.len() + 2;

/// One word of a sentence.
#[derive(Clone)]
pub struct Word {
    /// Position and length in chars.
    pub start: usize,
    pub len: usize,
    /// The entry read here; `None` for text no word covers.
    pub entry: Option<&'static ArchivedWordEntry>,
    /// The dictionary form looked up: 食べる for 食べさせられた.
    pub base: String,
    /// How the text is conjugated from `base`: causative, passive, past.
    pub reasons: Reasons,
    pub class: Class,
    /// Chars the text shares with `base` before its conjugated ending: 2
    /// for 食べ|させられた, 1 for 飲|みたい. `len` when not conjugated.
    pub stem: usize,
    /// Chars before where grammar attaches: the stem, and for a godan verb
    /// its row kana too (飲み|たい), for a noun with する its し
    /// (勉強し|ました). `len` when not conjugated.
    pub attach: usize,
    /// An expression JMdict lists whole that is a phrase of words, kanji
    /// and particle (と言う, 雨が降る): [`split_phrase`] gives its words.
    pub phrase: bool,
}

/// Verb forms the deinflection rules don't read, as `(dictionary form,
/// reasons, ichidan)`: the ます-stem alone (知り → 知る, 食べ → 食べる),
/// and the negatives ん (泣かん → 泣く) and ねば (書かねば → 書く). Each
/// form says whether it is an ichidan verb's or a godan one's, which the
/// entry must be.
fn extra_forms(surface: &str) -> Vec<(String, Reasons, bool)> {
    let mut out = Vec::new();
    // The u-row kana of an i-row or a-row one: 知り → る, 泣か → く.
    let u_of = |c: char| -> Option<char> {
        Some(match c {
            'い' | 'わ' => 'う',
            'き' | 'か' => 'く',
            'ぎ' | 'が' => 'ぐ',
            'し' | 'さ' => 'す',
            'ち' | 'た' => 'つ',
            'に' | 'な' => 'ぬ',
            'び' | 'ば' => 'ぶ',
            'み' | 'ま' => 'む',
            'り' | 'ら' => 'る',
            _ => return None,
        })
    };
    let a_row = |c: char| "わかがさたなばまら".contains(c);
    let godan = |stem: &str, reasons: Reasons, out: &mut Vec<(String, Reasons, bool)>| {
        if let Some((i, last)) = stem.char_indices().next_back()
            && let Some(u) = u_of(last)
        {
            out.push((format!("{}{u}", &stem[..i]), reasons, false));
        }
    };
    let masu = Reasons::of(&[Reason::MasuStem]);
    if surface.chars().next_back().is_some_and(|c| !a_row(c)) {
        godan(surface, masu, &mut out);
    }
    out.push((format!("{surface}る"), masu, true));
    for (ending, reasons) in [
        ("ん", Reasons::of(&[Reason::Negative])),
        (
            "ねば",
            Reasons::of(&[Reason::Negative, Reason::Conditional]),
        ),
    ] {
        let Some(stem) = surface.strip_suffix(ending).filter(|s| !s.is_empty()) else {
            continue;
        };
        if stem.chars().next_back().is_some_and(a_row) {
            godan(stem, reasons, &mut out);
        }
        out.push((format!("{stem}る"), reasons, true));
    }
    out
}

/// A dictionary entry as the lattice reads it.
pub type Entry = ArchivedWordEntry;

/// The verbs `head` is the ます-stem of, of the kind it fits: 見 (見る),
/// 遊び (遊ぶ), 迎え (迎える). Not 上 (上る is godan, whose stem would be
/// 上り), nor a kana stem of a verb usually written in kanji (うち, 打つ).
pub fn masu_stem_verbs(head: &str) -> Vec<&'static ArchivedWordEntry> {
    let masu = Reasons::of(&[Reason::MasuStem]);
    let mut out: Vec<&'static ArchivedWordEntry> = Vec::new();
    for (form, reasons, ichidan) in extra_forms(head) {
        if reasons != masu {
            continue;
        }
        for e in crate::lookup(&form) {
            let spelled = e.kanji_forms.iter().any(|k| k.text.as_str() == form)
                || e.reading_forms.iter().any(|r| r.text.as_str() == form);
            let kind = if ichidan {
                ichidan_only(e)
            } else {
                godan_only(e)
            };
            // A kana stem of a verb written in kanji is likelier a word of
            // its own: うち isn't 打つ's, とおり 通る's.
            let kana = form.chars().all(japanese_utils::is_kana);
            let usually_kana = e.senses.iter().any(|s| {
                s.misc
                    .iter()
                    .any(|m| matches!(m, ArchivedMisc::UsuallyKana))
            });
            let written = !kana || e.kanji_forms.is_empty() || usually_kana;
            if spelled && kind && written && !out.iter().any(|x| std::ptr::eq(*x, e)) {
                out.push(e);
            }
        }
    }
    out
}

/// Whether `e` is a noun taking する (勉強, 散歩): `vs` on a sense.
pub fn takes_suru(e: &ArchivedWordEntry) -> bool {
    e.senses
        .iter()
        .flat_map(|s| s.pos.iter())
        .any(|p| matches!(p, P::VerbSuru))
}

/// Whether `e` is an ichidan verb and no other kind of verb.
fn ichidan_only(e: &ArchivedWordEntry) -> bool {
    let mut ichidan = false;
    for p in e.senses.iter().flat_map(|s| s.pos.iter()) {
        if matches!(p, P::VerbIchidan | P::VerbIchidanS) {
            ichidan = true;
        } else if class_of(p) == Some(Class::Verb) {
            return false;
        }
    }
    ichidan
}

/// Whether `e` is a godan verb and no other kind.
fn godan_only(e: &ArchivedWordEntry) -> bool {
    let mut godan = false;
    for p in e.senses.iter().flat_map(|s| s.pos.iter()) {
        if godan_row(p).is_some() {
            godan = true;
        } else if class_of(p) == Some(Class::Verb) {
            return false;
        }
    }
    godan
}

/// [`Word::stem`] and [`Word::attach`] of a word read as `surface`.
fn stem_len(surface: &[char], node: &Node) -> (usize, usize) {
    let len = surface.len();
    let Some(e) = node.entry.filter(|_| !node.reasons.is_empty()) else {
        return (len, len);
    };
    // At least one char: 来た → 来る shares 来, but きた → くる nothing.
    let shared = surface
        .iter()
        .zip(node.base.chars())
        .take_while(|(a, b)| *a == b)
        .count()
        .clamp(1, len);
    let row_kana =
        (node.class == Class::Verb && godan_only(e)) || node.reasons.contains(Reason::SuruStem);
    (shared, (shared + usize::from(row_kana)).min(len))
}

fn class_of(p: &P) -> Option<Class> {
    Some(match p {
        P::Particle => Class::Particle,
        P::Conjunction => Class::Conjunction,
        P::Interjection => Class::Interjection,
        P::Noun
        | P::NounAdverbial
        | P::NounTemporal
        | P::NounProper
        | P::Pronoun
        | P::Numeric
        | P::AdjectiveNo => Class::Noun,
        P::AdjectiveNa => Class::NaAdjective,
        P::Adjective | P::AdjectiveFormal => Class::Adjective,
        P::Auxiliary | P::AuxiliaryAdjective | P::AuxiliaryVerb | P::Copula => Class::Auxiliary,
        P::Adverb | P::AdverbTo | P::AdjectiveT => Class::Adverb,
        P::Prefix | P::NounPrefix => Class::Prefix,
        P::Suffix | P::NounSuffix | P::Counter => Class::Suffix,
        P::AdjectivePrenominal => Class::Prenominal,
        P::Expression => Class::Expression,
        P::VerbIchidan
        | P::VerbIchidanS
        | P::VerbGodanBu
        | P::VerbGodanGu
        | P::VerbGodanKu
        | P::VerbGodanKuS
        | P::VerbGodanMu
        | P::VerbGodanNu
        | P::VerbGodanRu
        | P::VerbGodanRuIrr
        | P::VerbGodanSu
        | P::VerbGodanTsu
        | P::VerbGodanU
        | P::VerbGodanUS
        | P::VerbGodanUru
        | P::VerbSuru
        | P::VerbSuruS
        | P::VerbSuruC
        | P::VerbKuru
        | P::VerbNu
        | P::VerbRu
        | P::VerbUnclassified => Class::Verb,
        P::VerbTransitive | P::VerbIntransitive | P::Unknown => return None,
    })
}

/// The classes of `e`'s senses, in sense order, each once.
pub fn classes(e: &ArchivedWordEntry) -> Vec<Class> {
    let mut out = Vec::new();
    for c in e
        .senses
        .iter()
        .flat_map(|s| s.pos.iter().filter_map(class_of))
    {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn state(c: Class) -> usize {
    CLASSES.iter().position(|&k| k == c).unwrap_or(START)
}

/// A char a word can hold. Punctuation, digits and Latin letters end words.
fn wordy(c: char) -> bool {
    japanese_utils::is_kanji(c)
        || japanese_utils::is_kana(c)
        || matches!(c, '々' | '〆' | 'ヵ' | 'ヶ')
}

// ── Costs ────────────────────────────────────────────────────────────────

/// What each trait of a reading costs; lower is likelier. Every word pays
/// `word`, so fewer, longer words win unless they are rarer. Tuned against
/// the corpus by `tests/segmentation.rs` (`SEG_COSTS` overrides them).
#[derive(Debug, Clone)]
pub struct Costs {
    pub word: i32,
    /// Per sense before the one a class comes from: 歯 (noun) is は's later
    /// reading, after the particle.
    pub later_sense: i32,
    /// The form read has a frequency tag, but not a top one: news2, ichi2,
    /// gai2, or the rarest nf band (nf48; nf01 costs a 48th of it).
    pub some_freq: i32,
    /// The form read has no frequency tag.
    pub no_freq: i32,
    /// Kana text read as a word usually written in kanji: は as 歯.
    pub kana_for_kanji: i32,
    /// A lone kana that isn't a particle, auxiliary or affix.
    pub lone_kana: i32,
    /// One char no word covers.
    pub unknown_char: i32,
    /// A katakana run no word covers, read as one name or loanword.
    pub unknown_katakana: i32,
    /// An expression (それは, という): a breakdown would rather show its
    /// words, and the grammar layer names the pattern.
    pub expression: i32,
    /// An expression of kanji and particles, a phrase its words make up
    /// (と言う, 雨が降る): one word in JMdict, words to a learner.
    pub phrase: i32,
    /// A conjunction or interjection inside a clause: では after a noun is
    /// で|は.
    pub mid_conjunction: i32,
    /// A clause opening on a particle, auxiliary or suffix.
    pub clause_open: i32,
    pub particle_particle: i32,
    /// A word right after an interjection, with no punctuation between:
    /// 今日は家で is 今日|は, not the greeting.
    pub after_interjection: i32,
    /// Two nouns side by side: often one compound read in two.
    pub noun_noun: i32,
    /// An affix not where it attaches: a suffix after anything but a noun,
    /// a prefix before anything but a noun.
    pub stray_affix: i32,
    /// Text that is none of the entry's spellings, found only because the
    /// index folds kana: があ for ガー.
    pub kana_mismatch: i32,
    /// A kana content word without a top frequency tag opening on a
    /// particle's kana right after a noun or particle: は|どう read as
    /// 波動, は|めった, が|あ. (とても, もう are common: no cost.)
    pub particle_head: i32,
    /// The share, in percent, of the frequency cost one-kana particles and
    /// auxiliaries pay: JMdict tags few of them (ん, ぬ), common as they
    /// are.
    pub function_freq_percent: i32,
    /// A verb form only the lattice reads (the bare ます-stem, ん, ねば):
    /// where a noun is written the same (休み), the noun is likelier.
    pub extra_form: i32,
    /// An imperative that doesn't end its sentence (see [`mid_imperative`]).
    pub mid_imperative: i32,
    /// The frequency cost of a content word with an IPADIC word cost
    /// (`word_cost`, against its part of speech's median): `ipadic_base`
    /// plus `ipadic_scale` percent of it, at least 0, when that is less
    /// than its JMdict tags give. The tags still mark the commonest; the
    /// costs grade the rest (拠点 isn't as rare as an untagged word). An
    /// `ipadic_scale` of 0 keeps the tags alone.
    pub ipadic_base: i32,
    pub ipadic_scale: i32,
}

impl Default for Costs {
    fn default() -> Self {
        Self {
            word: 1000,
            later_sense: 100,
            some_freq: 500,
            no_freq: 1200,
            kana_for_kanji: 1000,
            lone_kana: 1500,
            unknown_char: 4000,
            unknown_katakana: 2500,
            expression: 0,
            phrase: 700,
            mid_conjunction: 600,
            clause_open: 800,
            particle_particle: 100,
            after_interjection: 1200,
            noun_noun: 300,
            stray_affix: 700,
            kana_mismatch: 1500,
            particle_head: 1000,
            function_freq_percent: 50,
            extra_form: 600,
            mid_imperative: 500,
            ipadic_base: 600,
            ipadic_scale: 15,
        }
    }
}

impl Costs {
    /// `self` with the `name=value` pairs of `spec` (comma-separated) set.
    pub fn with(mut self, spec: &str) -> Result<Self, String> {
        for pair in spec.split(',').filter(|p| !p.trim().is_empty()) {
            let (k, v) = pair
                .split_once('=')
                .ok_or_else(|| format!("not name=value: {pair}"))?;
            let v: i32 = v.trim().parse().map_err(|e| format!("{pair}: {e}"))?;
            let slot = match k.trim() {
                "word" => &mut self.word,
                "later_sense" => &mut self.later_sense,
                "some_freq" => &mut self.some_freq,
                "no_freq" => &mut self.no_freq,
                "kana_for_kanji" => &mut self.kana_for_kanji,
                "lone_kana" => &mut self.lone_kana,
                "unknown_char" => &mut self.unknown_char,
                "unknown_katakana" => &mut self.unknown_katakana,
                "expression" => &mut self.expression,
                "phrase" => &mut self.phrase,
                "mid_conjunction" => &mut self.mid_conjunction,
                "clause_open" => &mut self.clause_open,
                "particle_particle" => &mut self.particle_particle,
                "after_interjection" => &mut self.after_interjection,
                "noun_noun" => &mut self.noun_noun,
                "stray_affix" => &mut self.stray_affix,
                "kana_mismatch" => &mut self.kana_mismatch,
                "particle_head" => &mut self.particle_head,
                "function_freq_percent" => &mut self.function_freq_percent,
                "extra_form" => &mut self.extra_form,
                "mid_imperative" => &mut self.mid_imperative,
                "ipadic_base" => &mut self.ipadic_base,
                "ipadic_scale" => &mut self.ipadic_scale,
                other => return Err(format!("no cost named {other}")),
            };
            *slot = v;
        }
        Ok(self)
    }

    /// The cost of reading `node` after `prev` (a state).
    fn connection(&self, prev: usize, node: &Node) -> i32 {
        use Class as C;
        let prev = CLASSES.get(prev).copied();
        let head = if (node.particle_head
            && matches!(prev, Some(C::Noun | C::Suffix | C::Unknown | C::Particle)))
            || (node.kana_head && matches!(prev, Some(C::NaAdjective | C::Noun)))
        {
            self.particle_head
        } else {
            0
        };
        head + match (prev, node.class) {
            (None, C::Particle | C::Suffix | C::Auxiliary) => self.clause_open,
            (Some(_), C::Conjunction | C::Interjection) => self.mid_conjunction,
            // A particle takes の freely: などの, からの, での.
            (Some(C::Particle), C::Particle) if node.base == "の" => 0,
            (Some(C::Particle), C::Particle) => self.particle_particle,
            (Some(C::Interjection), _) => self.after_interjection,
            (Some(C::Prefix), C::Noun | C::Suffix | C::NaAdjective) => 0,
            (Some(C::Prefix), _) => self.stray_affix,
            (Some(C::Noun | C::Unknown), C::Suffix) => 0,
            (_, C::Suffix) => self.stray_affix,
            (Some(C::Noun), C::Noun) => self.noun_noun,
            (Some(C::Noun), _) if node.kanji_expression => self.noun_noun,
            _ => 0,
        }
    }

    /// The frequency cost of `e` written as `form` from its IPADIC word
    /// cost, if it has one (and [`Costs::ipadic_scale`] isn't 0).
    fn ipadic_freq(&self, e: &ArchivedWordEntry, form: &str) -> Option<i32> {
        if self.ipadic_scale == 0 {
            return None;
        }
        let cost = e
            .kanji_forms
            .iter()
            .find(|k| k.text.as_str() == form)
            .map(|k| &k.word_cost)
            .or_else(|| {
                e.reading_forms
                    .iter()
                    .find(|r| r.text.as_str() == form)
                    .map(|r| &r.word_cost)
            })?
            .as_ref()?
            .to_native();
        Some((self.ipadic_base + i32::from(cost) * self.ipadic_scale / 100).max(0))
    }

    /// The frequency tags of the form of `e` written as `form`.
    fn freq(&self, e: &ArchivedWordEntry, form: &str) -> i32 {
        let tags = e
            .kanji_forms
            .iter()
            .find(|k| k.text.as_str() == form)
            .map(|k| &k.priorities)
            .or_else(|| {
                e.reading_forms
                    .iter()
                    .find(|r| r.text.as_str() == form)
                    .map(|r| &r.priorities)
            });
        let Some(tags) = tags.filter(|t| !t.is_empty()) else {
            return self.no_freq;
        };
        // news1, ichi1, spec1/2, gai1: the commonest words.
        if tags.iter().any(|f| {
            !matches!(f.kind, ArchivedFreqKind::Nf)
                && (f.value == 1 || matches!(f.kind, ArchivedFreqKind::Spec))
        }) {
            return 0;
        }
        // nf01 (the 500 commonest) … nf48: graded up to `some_freq`.
        let nf = tags
            .iter()
            .filter(|f| matches!(f.kind, ArchivedFreqKind::Nf))
            .map(|f| i32::from(f.value))
            .min();
        match nf {
            Some(nf) => self.some_freq * nf / 48,
            None => self.some_freq,
        }
    }
}

/// Whether `e`, read as `class`, may stand conjugated as `surface` from
/// `base`: a deinflected form only counts for a word that conjugates, so
/// があ|る can't be a noun があ conjugated; and a godan verb only in its
/// own row, so はない isn't 張る (whose negative is はらない), which the
/// ichidan rule ない → る would make it.
fn conjugates(
    class: Class,
    e: &ArchivedWordEntry,
    reasons: Reasons,
    base: &str,
    surface: &str,
) -> bool {
    // Verbs of state have no potential: 違えた is 違える, not 違う's.
    const NO_POTENTIAL: [&str; 12] = [
        "違う",
        "ちがう",
        "分かる",
        "わかる",
        "要る",
        "ある",
        "有る",
        "在る",
        "できる",
        "出来る",
        "見える",
        "聞こえる",
    ];
    if reasons.contains(Reason::Potential) && NO_POTENTIAL.contains(&base) {
        return false;
    }
    // てくれた is て + くれる "give": 来る's potential is これる, こられる,
    // not the godan-looking くれる (与えてくれた isn't 与えてくる's).
    if reasons.contains_all(Reasons::of(&[Reason::Potential, Reason::TeKuru]))
        && (surface.contains("てくれ") || surface.contains("でくれ"))
    {
        return false;
    }
    match class {
        // An auxiliary that is a verb too (成る) in its own row: なよ isn't
        // なる's.
        Class::Verb | Class::Auxiliary => verb_fits(e, base, surface),
        Class::Adjective | Class::Expression => true,
        // 勉強して → 勉強, a noun taking する.
        Class::Noun | Class::NaAdjective => {
            reasons.contains(Reason::SuruStem)
                && e.senses.iter().any(|s| {
                    s.pos
                        .iter()
                        .any(|p| matches!(p, P::VerbSuru | P::VerbSuruS))
                })
        }
        _ => false,
    }
}

/// The kana a godan verb's stem goes on with, by the verb's type; `None`
/// for a type that isn't godan or isn't checked.
fn godan_row(p: &P) -> Option<&'static str> {
    Some(match p {
        P::VerbGodanU | P::VerbGodanUS => "わいうえおっ",
        P::VerbGodanKu => "かきくけこい",
        P::VerbGodanKuS => "かきくけこっ",
        P::VerbGodanGu => "がぎぐげごい",
        P::VerbGodanSu => "さしすせそ",
        P::VerbGodanTsu => "たちつてとっ",
        P::VerbGodanNu => "なにぬねのん",
        P::VerbGodanBu => "ばびぶべぼん",
        P::VerbGodanMu => "まみむめもん",
        P::VerbGodanRu | P::VerbGodanRuIrr => "らりるれろっ",
        _ => return None,
    })
}

/// Whether `surface` can be the verb `e` conjugated from `base`. A verb
/// that is godan and nothing else must go on from its stem (`base` less
/// its last kana) with a kana of its row; one that is ichidan and nothing
/// else never with っ, り or ん (はめった isn't はめる's past: はめた is).
fn verb_fits(e: &ArchivedWordEntry, base: &str, surface: &str) -> bool {
    let mut rows = Vec::new();
    let mut ichidan = false;
    for p in e.senses.iter().flat_map(|s| s.pos.iter()) {
        match godan_row(p) {
            Some(row) => rows.push(row),
            None if matches!(p, P::VerbIchidan | P::VerbIchidanS) => ichidan = true,
            // Another verb type (する, くる…): no check.
            None if class_of(p) == Some(Class::Verb) => return true,
            None => {}
        }
    }
    if rows.is_empty() == !ichidan {
        // Not a verb, or both godan and ichidan: no check.
        return true;
    }
    let Some((last, _)) = base.char_indices().next_back() else {
        return true;
    };
    let stem = &base[..last];
    let Some(next) = surface.strip_prefix(stem).and_then(|r| r.chars().next()) else {
        // Spelled apart from its base (ある → ない), or nothing after the
        // stem: nothing to check.
        return true;
    };
    if ichidan {
        return !matches!(next, 'っ' | 'り' | 'ん');
    }
    rows.iter().any(|row| row.contains(next))
}

/// One candidate word of the lattice.
struct Node {
    len: usize,
    cost: i32,
    class: Class,
    entry: Option<&'static ArchivedWordEntry>,
    base: String,
    reasons: Reasons,
    /// A kana content word whose first kana is a particle's (はどう).
    particle_head: bool,
    /// The same for a word written in kana that takes kanji, whatever its
    /// frequency: after a noun or な adjective, に and で are its own
    /// (になった is the expression になる, or に|なった; not 担う).
    kana_head: bool,
    /// A verb expression that opens on a kanji word (拠点とする, 姿を消す):
    /// after a noun it joins it as that word would, so it pays
    /// [`Costs::noun_noun`] too, and 貿易|拠点として isn't cheaper than
    /// 貿易|拠点|として. Not one that stays a noun (年ぶり, after digits).
    kanji_expression: bool,
    /// See [`Word::phrase`].
    phrase: bool,
}

/// The candidates for one dictionary form of `surface`, one per class its
/// senses have.
fn push_nodes(
    costs: &Costs,
    out: &mut Vec<Node>,
    entries: &[&'static ArchivedWordEntry],
    surface: &str,
    len: usize,
    form: &str,
    reasons: Reasons,
) {
    let kana = surface.chars().all(japanese_utils::is_kana);
    let lone_kana = kana && len == 1;
    let particle_head = kana
        && len >= 2
        && surface.starts_with(['は', 'が', 'を', 'に', 'で', 'と', 'も', 'へ', 'の']);
    for &e in entries {
        // A form already read for this entry by a less conjugated path.
        if out
            .iter()
            .any(|n| n.len == len && n.entry.is_some_and(|x| std::ptr::eq(x, e)))
        {
            continue;
        }
        let mut seen: Vec<Class> = Vec::new();
        let usually_kana = e.senses.iter().any(|s| {
            s.misc
                .iter()
                .any(|m| matches!(m, ArchivedMisc::UsuallyKana))
        });
        let kana_for_kanji =
            kana && !e.kanji_forms.is_empty() && !usually_kana && form_is_reading(e, form);
        let spelled = e.kanji_forms.iter().any(|k| k.text.as_str() == form)
            || e.reading_forms.iter().any(|r| r.text.as_str() == form);
        let freq = costs.freq(e, form);
        let entry_classes = classes(e);
        let expression = entry_classes.contains(&Class::Expression);
        // An expression (気がつく) keeps the tags: IPADIC's cost is its
        // verb's, and made cheap it swallows the noun before (電|気がつく).
        let ipadic_freq = costs.ipadic_freq(e, form).filter(|_| !expression);
        // A phrase JMdict lists whole, kanji and particle (と言う, 雨が降る,
        // 時には): the breakdown would rather show its words, and the
        // grammar layer names the pattern. Kana ones (じゃない, ほうがいい)
        // stay whole.
        // A conjunction too: と言った "and yet" is と|言った to a learner.
        let phrase = (expression || entry_classes.contains(&Class::Conjunction))
            && surface.chars().any(japanese_utils::is_kanji)
            && !phrase_particles(surface).is_empty();
        let kanji_expression = surface.starts_with(japanese_utils::is_kanji)
            && expression
            && entry_classes.contains(&Class::Verb);
        let base_cost = costs.word + if spelled { 0 } else { costs.kana_mismatch };
        for (rank, s) in e.senses.iter().enumerate() {
            // `vs` (merged with する's own `vs-i`) tags a noun taking する:
            // 破滅 is no verb, so はめった isn't it conjugated as one. Its
            // する forms come through the SuruStem rule.
            let pos = s.pos.iter().filter(|p| {
                !matches!(p, P::VerbSuru) || form.ends_with("する") || form.ends_with("ずる")
            });
            for class in pos.filter_map(class_of) {
                if seen.contains(&class)
                    || (!reasons.is_empty() && !conjugates(class, e, reasons, form, surface))
                {
                    continue;
                }
                seen.push(class);
                let lone = lone_kana
                    && !matches!(
                        class,
                        Class::Particle | Class::Auxiliary | Class::Suffix | Class::Prefix
                    );
                let function = len == 1 && matches!(class, Class::Particle | Class::Auxiliary);
                // Only nouns, verbs and adjectives take IPADIC's costs: they
                // compare within a part of speech, and an adverb (時に, 本に)
                // or a conjunction (なので, だから) made cheap beats the noun
                // and particle it competes with. Particles and endings keep
                // JMdict's tags: their costs only make sense with MeCab's
                // own connection costs.
                let content_freq = match ipadic_freq {
                    Some(f)
                        if matches!(
                            class,
                            Class::Noun | Class::Verb | Class::Adjective | Class::NaAdjective
                        ) =>
                    {
                        f.min(freq)
                    }
                    _ => freq,
                };
                out.push(Node {
                    len,
                    cost: base_cost
                        + if function {
                            freq * costs.function_freq_percent / 100
                        } else {
                            content_freq
                        }
                        + costs.later_sense * (rank.min(3) as i32)
                        + if lone { costs.lone_kana } else { 0 }
                        // Function words are written in kana anyway: the
                        // particle の has 乃 and 之 too.
                        + if kana_for_kanji && content(class) {
                            costs.kana_for_kanji
                        } else {
                            0
                        }
                        + if class == Class::Expression { costs.expression } else { 0 }
                        + if phrase { costs.phrase } else { 0 },
                    class,
                    entry: Some(e),
                    base: form.to_owned(),
                    reasons,
                    particle_head: particle_head && content(class) && freq > 0,
                    kana_head: particle_head
                        && content(class)
                        && class != Class::Expression
                        && kana_for_kanji,
                    kanji_expression,
                    phrase,
                });
            }
        }
    }
}

/// A word with a meaning of its own, as opposed to a function word.
fn content(class: Class) -> bool {
    matches!(
        class,
        Class::Noun
            | Class::NaAdjective
            | Class::Verb
            | Class::Adjective
            | Class::Adverb
            | Class::Prefix
            | Class::Suffix
            | Class::Expression
    )
}

fn form_is_reading(e: &ArchivedWordEntry, form: &str) -> bool {
    e.reading_forms.iter().any(|r| r.text.as_str() == form)
        && !e.kanji_forms.iter().any(|k| k.text.as_str() == form)
}

/// Adds [`Costs::mid_imperative`] to the imperatives among `nodes` that
/// don't end their sentence: `rest` is the text from where they start.
/// An imperative ends it, or comes before と, って, よ, な, ね, ぞ (止まれと,
/// 来いよ); 実行され、 is 実行 + される's stem, not さる's imperative.
fn mid_imperative(costs: &Costs, nodes: &mut [Node], rest: &[char]) {
    for n in nodes {
        if !n.reasons.contains(Reason::Imperative) {
            continue;
        }
        let ends = rest
            .get(n.len)
            .is_none_or(|c| "。！？!?」』）)\n…".contains(*c) || "とっよなねぞや".contains(*c));
        if !ends {
            n.cost += costs.mid_imperative;
        }
    }
}

/// Every candidate word at the start of `run`, the rest of a run of word
/// chars.
fn candidates(
    costs: &Costs,
    run: &[char],
    walk: &mut FstWalk,
    deinflector: &mut deinflect::Deinflector,
) -> Vec<Node> {
    let window: String = run.iter().take(MAX_WORD_CHARS).collect();
    // Byte offset past each prefix of the window, by its length in chars.
    let ends: Vec<usize> = window
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .collect();
    walk.walk(&window);
    let mut out = Vec::new();
    for (i, &byte_end) in ends.iter().enumerate().rev() {
        let len = i + 1;
        let Some(surface) = window.get(..byte_end) else {
            continue;
        };
        // A godan verb's a-row stem stands alone before ざる (書か|ざるを得
        // ない): the stem of its ん negative.
        let before_zaru = run.get(len..).is_some_and(|r| r.starts_with(&['ざ', 'る']));
        // A common noun spelled the same in kana (とき as 時, not 解く's
        // stem) is likelier still than its kana costs it.
        let kana_noun = surface.chars().all(japanese_utils::is_kana)
            && walk
                .get(&window, surface)
                .and_then(get_entry_group)
                .is_some_and(|indices| {
                    indices.iter().filter_map(|&i| get_entry(i)).any(|e| {
                        classes(e).contains(&Class::Noun)
                            && (costs.freq(e, surface) == 0
                                || e.kanji_forms
                                    .iter()
                                    .any(|k| costs.freq(e, k.text.as_str()) == 0))
                    })
                });
        let mut forms = extra_forms(surface);
        if before_zaru {
            forms.extend(
                extra_forms(&format!("{surface}ん"))
                    .into_iter()
                    .filter(|&(_, _, ichidan)| !ichidan),
            );
        }
        for (form, reasons, ichidan) in forms {
            let Some(indices) = walk.get(&window, &form).and_then(get_entry_group) else {
                continue;
            };
            let entries: Vec<_> = indices
                .iter()
                .filter_map(|&i| get_entry(i))
                .filter(|e| {
                    if ichidan {
                        ichidan_only(e)
                    } else {
                        godan_only(e)
                    }
                })
                .collect();
            let from = out.len();
            push_nodes(costs, &mut out, &entries, surface, len, &form, reasons);
            for n in out.get_mut(from..).into_iter().flatten() {
                n.cost += costs.extra_form + if kana_noun { costs.kana_for_kanji } else { 0 };
            }
        }
        if walk.reaches(byte_end) || deinflect::can_deinflect(surface) {
            for (form, reasons) in deinflector.run_with_reasons(surface) {
                let Some(group) = walk.get(&window, form) else {
                    continue;
                };
                let Some(indices) = get_entry_group(group) else {
                    continue;
                };
                let entries: Vec<_> = indices.iter().filter_map(|&i| get_entry(i)).collect();
                push_nodes(costs, &mut out, &entries, surface, len, form, reasons);
            }
        }
        // The stem of a conjugated verb, which conjugates like an ichidan
        // one: 言われ (言われる, 言う's passive), 食べさせ. Written Japanese
        // ends clauses on it (…とも言われ、).
        // After the plain readings, which win for the same entry (止まれ is
        // 止まる's imperative before the stem of its potential), and only
        // the forms that make such a verb.
        let masu = Reasons::of(&[Reason::MasuStem]);
        let ichidan_like = Reasons::of(&[Reason::Passive, Reason::Potential, Reason::Causative]);
        let stem_form = format!("{surface}る");
        for (form, reasons) in deinflector.run_with_reasons(&stem_form) {
            if form == stem_form || reasons.is_empty() || !ichidan_like.contains_all(reasons) {
                continue;
            }
            let Some(indices) = walk.get(&window, form).and_then(get_entry_group) else {
                continue;
            };
            let entries: Vec<_> = indices.iter().filter_map(|&i| get_entry(i)).collect();
            let from = out.len();
            push_nodes(
                costs,
                &mut out,
                &entries,
                surface,
                len,
                form,
                reasons.union(masu),
            );
            for n in out.get_mut(from..).into_iter().flatten() {
                n.cost += costs.extra_form;
            }
        }
    }
    // Text no word covers still has to be read: one char, or a whole
    // katakana run as one unknown word.
    let katakana = run
        .iter()
        .take_while(|&&c| japanese_utils::is_katakana(c))
        .count();
    if katakana > 1 && !out.iter().any(|n| n.len == katakana) {
        out.push(unknown(run, katakana, costs.unknown_katakana));
    }
    out.push(unknown(run, 1, costs.unknown_char));
    out
}

/// The first `len` chars of `run` as a word no entry covers.
fn unknown(run: &[char], len: usize, cost: i32) -> Node {
    Node {
        len,
        cost,
        class: Class::Unknown,
        entry: None,
        base: run.iter().take(len).collect(),
        reasons: Reasons::EMPTY,
        particle_head: false,
        kana_head: false,
        kanji_expression: false,
        phrase: false,
    }
}

/// The best way to reach a (position, state): its cost and where from.
#[derive(Clone, Copy)]
struct Best {
    cost: i32,
    from: (usize, usize),
    /// The candidate read to get here, as (start, index in its list);
    /// `None` for a step over a char no word holds.
    node: Option<(usize, usize)>,
}

/// The best ways into each (position, state) of the lattice.
struct Table(Vec<[Option<Best>; STATES]>);

impl Table {
    fn get(&self, pos: usize, st: usize) -> Option<Best> {
        self.0
            .get(pos)
            .and_then(|row| row.get(st))
            .copied()
            .flatten()
    }

    /// Keep `b` as the way into (pos, st) if it beats the one there.
    fn relax(&mut self, pos: usize, st: usize, b: Best) {
        if let Some(slot) = self.0.get_mut(pos).and_then(|row| row.get_mut(st))
            && slot.is_none_or(|old| b.cost < old.cost)
        {
            *slot = Some(b);
        }
    }
}

/// The words of `sentence`, in order. Text outside words (punctuation,
/// digits, Latin letters) isn't listed; Japanese text no dictionary word
/// covers is, with no entry.
pub fn segment(sentence: &str) -> Vec<Word> {
    segment_with(sentence, &Costs::default())
}

/// The candidates the lattice weighs at char `at` of `sentence`, as
/// `(len, class, cost, headword)`: for tuning the costs.
#[doc(hidden)]
pub fn candidates_at(sentence: &str, at: usize, costs: &Costs) -> Vec<(usize, Class, i32, String)> {
    let chars: Vec<char> = sentence.chars().collect();
    let rest = chars.get(at..).unwrap_or_default();
    let run_len = rest.iter().take_while(|&&c| wordy(c)).count();
    let run = rest.get(..run_len).unwrap_or_default();
    if run.is_empty() {
        return Vec::new();
    }
    let mut walk = FstWalk::default();
    let mut deinflector = deinflect::Deinflector::default();
    let mut found = candidates(costs, run, &mut walk, &mut deinflector);
    mid_imperative(costs, &mut found, rest);
    found
        .into_iter()
        .map(|n| {
            let head = n
                .entry
                .map_or_else(|| n.base.clone(), |e| e.headword().to_owned());
            (n.len, n.class, n.cost, head)
        })
        .collect()
}

/// Where a phrase's particles may be, in chars: one opening it or right
/// after a kanji, with more after it (not the と of 長いこと).
fn phrase_particles(surface: &str) -> Vec<usize> {
    let c: Vec<char> = surface.chars().collect();
    let before = std::iter::once(None).chain(c.iter().copied().map(Some));
    c.iter()
        .zip(before)
        .take(c.len().saturating_sub(1))
        .enumerate()
        .filter(|&(_, (&here, before))| {
            matches!(here, 'が' | 'を' | 'に' | 'と' | 'で' | 'は' | 'も')
                && before.is_none_or(japanese_utils::is_kanji)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The words of a phrase [`segment`] reads as one expression
/// ([`Word::phrase`]): 雨|が|降って for 雨が降って, positions within
/// `surface`. `None` unless it splits into known words that aren't
/// phrases themselves, one a particle where [`phrase_particles`] puts one
/// (or a kana expression opening on it):
/// 何で|も isn't 何でも, nor 上|がった 上がった.
///
/// `after` is the text following it, read along so the last word is what
/// it is in the sentence: して of 気がしてならない is する's, not the
/// particle して.
pub fn split_phrase(surface: &str, after: &str) -> Option<Vec<Word>> {
    let costs = Costs {
        phrase: 100_000,
        ..Costs::default()
    };
    let n = surface.chars().count();
    let mut words = segment_with(&format!("{surface}{after}"), &costs);
    words.retain(|w| w.start < n);
    let whole = words.iter().map(|w| w.len).sum::<usize>() == n
        && words.last().is_some_and(|w| w.start + w.len == n);
    let at = phrase_particles(surface);
    let chars: Vec<char> = surface.chars().collect();
    // A particle, or one of the kana expressions opening on one (とは,
    // にも, ではない).
    let particle = words.iter().any(|w| {
        at.contains(&w.start)
            && match w.class {
                Class::Particle => true,
                Class::Expression | Class::Auxiliary => chars
                    .get(w.start..w.start + w.len)
                    .is_some_and(|c| c.iter().all(|&c| japanese_utils::is_kana(c))),
                _ => false,
            }
    });
    (whole && particle && words.len() >= 2 && words.iter().all(|w| w.entry.is_some() && !w.phrase))
        .then_some(words)
}

/// [`segment`] with other costs, for tuning them.
pub fn segment_with(sentence: &str, costs: &Costs) -> Vec<Word> {
    let chars: Vec<char> = sentence.chars().collect();
    let n = chars.len();
    let mut walk = FstWalk::default();
    let mut deinflector = deinflect::Deinflector::default();
    // The candidates at each char; none where no word can start.
    let mut nodes: Vec<Vec<Node>> = Vec::with_capacity(n);
    for (at, &c) in chars.iter().enumerate() {
        let rest = chars.get(at..).unwrap_or_default();
        if !wordy(c) {
            nodes.push(Vec::new());
            continue;
        }
        let run_len = rest.iter().take_while(|&&c| wordy(c)).count();
        let run = rest.get(..run_len).unwrap_or_default();
        let mut here = candidates(costs, run, &mut walk, &mut deinflector);
        mid_imperative(costs, &mut here, rest);
        nodes.push(here);
    }

    let mut best = Table(vec![[None; STATES]; n + 1]);
    best.relax(
        0,
        START,
        Best {
            cost: 0,
            from: (0, START),
            node: None,
        },
    );
    for (at, (&c, here_nodes)) in chars.iter().zip(&nodes).enumerate() {
        for st in 0..STATES {
            let Some(here) = best.get(at, st) else {
                continue;
            };
            if !wordy(c) {
                let next = if c.is_ascii_digit() || ('０'..='９').contains(&c) {
                    AFTER_NUMBER
                } else {
                    START
                };
                let step = Best {
                    cost: here.cost,
                    from: (at, st),
                    node: None,
                };
                best.relax(at + 1, next, step);
                continue;
            }
            let prev = if st == AFTER_NUMBER {
                state(Class::Noun)
            } else {
                st
            };
            for (i, node) in here_nodes.iter().enumerate() {
                let step = Best {
                    cost: here.cost + costs.connection(prev, node) + node.cost,
                    from: (at, st),
                    node: Some((at, i)),
                };
                // A する noun with its する is a verb to what follows:
                // アクセスできない|とき is a verb's とき.
                let next = if node.reasons.contains(Reason::SuruStem) {
                    state(Class::Verb)
                } else {
                    state(node.class)
                };
                best.relax(at + node.len, next, step);
            }
        }
    }

    let Some(mut st) = (0..STATES)
        .filter_map(|s| best.get(n, s).map(|b| (s, b.cost)))
        .min_by_key(|&(_, cost)| cost)
        .map(|(s, _)| s)
    else {
        return Vec::new();
    };
    let mut pos = n;
    let mut out = Vec::new();
    while pos > 0 {
        let Some(b) = best.get(pos, st) else {
            break;
        };
        if let Some(node) = b
            .node
            .and_then(|(start, i)| nodes.get(start).and_then(|ns| ns.get(i)))
        {
            let start = b.from.0;
            let surface = chars.get(start..start + node.len).unwrap_or_default();
            let (stem, attach) = stem_len(surface, node);
            out.push(Word {
                start,
                len: node.len,
                entry: node.entry,
                base: node.base.clone(),
                reasons: node.reasons,
                class: node.class,
                stem,
                attach,
                phrase: node.phrase,
            });
        }
        (pos, st) = b.from;
    }
    out.reverse();
    out
}
