//! What a deinflection rule undoes, and the set of them along a chain.

/// One step a rule undoes: 食べなかった → 食べる is [`Negative`] and
/// [`Past`].
///
/// [`Negative`]: Reason::Negative
/// [`Past`]: Reason::Past
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Reason {
    Negative,
    Past,
    Polite,
    /// The て form (食べて).
    TeForm,
    /// ば (食べれば).
    Conditional,
    /// たら (食べたら).
    Tara,
    /// たり (食べたり).
    Tari,
    Passive,
    /// Also the ichidan られる and 来られる, which are passive too.
    Potential,
    Causative,
    Imperative,
    /// 行こう, 食べよう, 行きましょう.
    Volitional,
    /// たい.
    Desiderative,
    /// ている.
    Progressive,
    /// てしまう.
    Completion,
    /// てみる.
    TryDoing,
    /// ておく.
    TeOku,
    /// てある.
    TeAru,
    /// てくる.
    TeKuru,
    /// ていく.
    TeIku,
    /// てほしい.
    TeHoshii,
    /// なさい (食べなさい).
    Nasai,
    /// The literary ず negative, beside [`Reason::Negative`].
    Literary,
    /// ずに.
    WithoutDoing,
    /// The く adverb of an adjective (高く).
    Adverbial,
    /// The さ noun of an adjective (高さ).
    Nominalization,
    /// する taken off a suru noun (勉強する → 勉強).
    SuruStem,
    /// The ます-stem standing alone (知りつつ, 食べに行く). No rule of this
    /// crate gives it, as any word ending in an i-row kana would then
    /// read as a verb; the sentence lattice (jmdict-core) tries it where
    /// a verb is likely.
    MasuStem,
}

impl Reason {
    pub const ALL: [Reason; 28] = [
        Self::Negative,
        Self::Past,
        Self::Polite,
        Self::TeForm,
        Self::Conditional,
        Self::Tara,
        Self::Tari,
        Self::Passive,
        Self::Potential,
        Self::Causative,
        Self::Imperative,
        Self::Volitional,
        Self::Desiderative,
        Self::Progressive,
        Self::Completion,
        Self::TryDoing,
        Self::TeOku,
        Self::TeAru,
        Self::TeKuru,
        Self::TeIku,
        Self::TeHoshii,
        Self::Nasai,
        Self::Literary,
        Self::WithoutDoing,
        Self::Adverbial,
        Self::Nominalization,
        Self::SuruStem,
        Self::MasuStem,
    ];

    /// The reason's name in data files, kebab-case: `te-form`,
    /// `adverbial`, `masu-stem`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Negative => "negative",
            Self::Past => "past",
            Self::Polite => "polite",
            Self::TeForm => "te-form",
            Self::Conditional => "conditional",
            Self::Tara => "tara",
            Self::Tari => "tari",
            Self::Passive => "passive",
            Self::Potential => "potential",
            Self::Causative => "causative",
            Self::Imperative => "imperative",
            Self::Volitional => "volitional",
            Self::Desiderative => "desiderative",
            Self::Progressive => "progressive",
            Self::Completion => "completion",
            Self::TryDoing => "try-doing",
            Self::TeOku => "te-oku",
            Self::TeAru => "te-aru",
            Self::TeKuru => "te-kuru",
            Self::TeIku => "te-iku",
            Self::TeHoshii => "te-hoshii",
            Self::Nasai => "nasai",
            Self::Literary => "literary",
            Self::WithoutDoing => "without-doing",
            Self::Adverbial => "adverbial",
            Self::Nominalization => "nominalization",
            Self::SuruStem => "suru-stem",
            Self::MasuStem => "masu-stem",
        }
    }

    /// The reason named `name` ([`Reason::name`]).
    pub fn from_name(name: &str) -> Option<Reason> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }

    const fn bit(self) -> u32 {
        1 << self as u8
    }
}

/// A set of [`Reason`]s: every step of the rules that led to a form, and of
/// every path that reached it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Reasons(u32);

impl Reasons {
    pub const EMPTY: Reasons = Reasons(0);

    pub const fn of(reasons: &[Reason]) -> Reasons {
        let mut bits = 0;
        let mut rest = reasons;
        while let [first, tail @ ..] = rest {
            bits |= first.bit();
            rest = tail;
        }
        Reasons(bits)
    }

    pub const fn union(self, other: Reasons) -> Reasons {
        Reasons(self.0 | other.0)
    }

    pub const fn contains(self, reason: Reason) -> bool {
        self.0 & reason.bit() != 0
    }

    /// Whether every reason of `other` is also in `self`.
    pub const fn contains_all(self, other: Reasons) -> bool {
        other.0 & !self.0 == 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Reason> {
        Reason::ALL.into_iter().filter(move |&r| self.contains(r))
    }
}

impl FromIterator<Reason> for Reasons {
    fn from_iter<I: IntoIterator<Item = Reason>>(iter: I) -> Self {
        iter.into_iter()
            .fold(Reasons::EMPTY, |acc, r| acc.union(Reasons::of(&[r])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_each_reason_once_in_order() {
        for (i, r) in Reason::ALL.iter().enumerate() {
            assert_eq!(*r as usize, i, "{r:?}");
        }
    }

    #[test]
    fn names_read_back() {
        for r in Reason::ALL {
            assert_eq!(Reason::from_name(r.name()), Some(r));
        }
        assert_eq!(Reason::from_name("nope"), None);
    }

    #[test]
    fn set_operations() {
        let np = Reasons::of(&[Reason::Negative, Reason::Past]);
        assert!(np.contains(Reason::Past));
        assert!(!np.contains(Reason::Polite));
        assert!(np.contains_all(Reasons::of(&[Reason::Negative])));
        assert!(!np.contains_all(Reasons::of(&[Reason::Polite])));
        assert_eq!(
            np.iter().collect::<Vec<_>>(),
            [Reason::Negative, Reason::Past]
        );
        assert_eq!(np, [Reason::Past, Reason::Negative].into_iter().collect());
        assert!(Reasons::EMPTY.is_empty());
    }
}
