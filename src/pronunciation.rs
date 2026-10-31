//! Dictionary-backed pronunciations with optional productive rules.
//!
//! This module owns pronunciation policy, provenance, and rule application.
//! The dictionary supplies stored pronunciations through `PronouncingDictionary`.

use std::borrow::Cow;

use crate::{
    dictionary::{PronouncingDictionary, Pronunciations},
    phoneme::Phoneme,
};

mod inflection;

pub use inflection::InflectionRule;
use inflection::pronunciation_derivations;

/// Whether words in running text may use pronunciations inferred from dictionary entries.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PronunciationMode {
    /// Only dictionary entries, allowing typographic apostrophe normalization.
    #[default]
    DictionaryOnly,
    /// Fall back to productive rules when no dictionary entry exists.
    AllowDerived,
}

/// A dictionary with an explicit pronunciation policy. The underlying dictionary
/// can be borrowed, so strict and permissive consumers can share the same data.
pub struct ConfiguredDictionary<D> {
    dict: D,
    mode: PronunciationMode,
}

/// Where a word's pronunciation came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PronunciationOrigin {
    /// The word matched a dictionary entry. `normalized` is true when a
    /// typographic apostrophe was converted to the dictionary's straight one.
    Dictionary { normalized: bool },
    /// The pronunciation was inferred from a dictionary base by applying one
    /// or more productive rules, ordered from the base outward.
    Derived {
        dictionary_base: String,
        rules: Vec<InflectionRule>,
    },
}

/// A word's pronunciations together with whether they came directly from the
/// dictionary or were inferred by productive rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordPronunciations<'d> {
    pub pronunciations: Cow<'d, [Box<[Phoneme]>]>,
    pub origin: PronunciationOrigin,
}

impl<D> ConfiguredDictionary<D> {
    pub(crate) fn new(dict: D, mode: PronunciationMode) -> Self {
        Self { dict, mode }
    }
}

/// Implements the dictionary trait's policy-aware pronunciation method.
pub(crate) fn pronunciations_for<'d, D: PronouncingDictionary + ?Sized>(
    dict: &'d D,
    word: &str,
) -> Option<WordPronunciations<'d>> {
    if let Some(pronunciations) = dict.lookup(word) {
        return Some(WordPronunciations {
            pronunciations: Cow::Borrowed(pronunciations),
            origin: PronunciationOrigin::Dictionary { normalized: false },
        });
    }

    // CMUdict uses straight apostrophes, while prose editors commonly insert
    // typographic ones. Treat the latter as the same exact headword before
    // falling back to a derived pronunciation.
    let normalized = word.replace('’', "'");
    if normalized != word
        && let Some(pronunciations) = dict.lookup(&normalized)
    {
        return Some(WordPronunciations {
            pronunciations: Cow::Borrowed(pronunciations),
            origin: PronunciationOrigin::Dictionary { normalized: true },
        });
    }

    if dict.pronunciation_mode() == PronunciationMode::DictionaryOnly {
        return None;
    }

    let derived = derive_pronunciations(dict, word)?;
    Some(WordPronunciations {
        pronunciations: Cow::Owned(derived.pronunciations),
        origin: PronunciationOrigin::Derived {
            dictionary_base: derived.dictionary_base,
            rules: derived.rules,
        },
    })
}

impl<D: PronouncingDictionary> PronouncingDictionary for ConfiguredDictionary<D> {
    fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
        self.dict.lookup(word)
    }

    fn pronunciation_mode(&self) -> PronunciationMode {
        self.mode
    }

    fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
        self.dict.entries()
    }
}

struct DerivedPronunciations {
    pronunciations: Pronunciations,
    dictionary_base: String,
    rules: Vec<InflectionRule>,
}

fn derive_pronunciations<D: PronouncingDictionary + ?Sized>(
    dict: &D,
    word: &str,
) -> Option<DerivedPronunciations> {
    for derivation in pronunciation_derivations(word) {
        for base in derivation.base_candidates() {
            let base_pronunciations = dictionary_base_pronunciations(dict, base)
                .or_else(|| derive_pronunciations(dict, base));
            if let Some(mut base_pronunciations) = base_pronunciations {
                let pronunciations = derivation.apply(&base_pronunciations.pronunciations);
                if pronunciations.is_empty() {
                    continue;
                }
                base_pronunciations.pronunciations = pronunciations;
                base_pronunciations.rules.push(derivation.rule());
                return Some(base_pronunciations);
            }
        }
    }

    None
}

fn dictionary_base_pronunciations<D: PronouncingDictionary + ?Sized>(
    dict: &D,
    base: &str,
) -> Option<DerivedPronunciations> {
    if let Some(pronunciations) = dict.lookup(base) {
        return Some(DerivedPronunciations {
            pronunciations: pronunciations.to_owned(),
            dictionary_base: base.to_string(),
            rules: Vec::new(),
        });
    }

    let normalized = base.replace('’', "'");
    (normalized != base)
        .then(|| dict.lookup(&normalized))
        .flatten()
        .map(|pronunciations| DerivedPronunciations {
            pronunciations: pronunciations.to_owned(),
            dictionary_base: normalized,
            rules: Vec::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmuDictionary;
    use std::sync::OnceLock;

    fn dict() -> &'static CmuDictionary {
        static DICT: OnceLock<CmuDictionary> = OnceLock::new();
        DICT.get_or_init(|| {
            CmuDictionary::load("data/CMUdict/cmudict-0.7b").expect("failed to load dictionary")
        })
    }

    #[test]
    fn dictionary_only_is_default_and_derivation_is_an_explicit_opt_in() {
        let strict = dict();
        assert_eq!(
            strict.pronunciation_mode(),
            PronunciationMode::DictionaryOnly
        );
        for word in ["kettle's", "kettle’s", "sorest"] {
            assert!(
                strict.pronunciations_for(word).is_none(),
                "unexpected {word}"
            );
        }

        let permissive = strict.with_pronunciation_mode(PronunciationMode::AllowDerived);
        for word in ["kettle's", "kettle’s", "sorest"] {
            assert!(permissive.lookup(word).is_none());
            assert!(matches!(
                permissive.pronunciations_for(word).unwrap().origin,
                PronunciationOrigin::Derived { .. }
            ));
        }
        assert_eq!(permissive.entries().count(), strict.entries().count());
        assert!(strict.pronunciations_for("kettle's").is_none());

        let strict_again = (&permissive).with_pronunciation_mode(PronunciationMode::DictionaryOnly);
        assert!(strict_again.pronunciations_for("kettle's").is_none());
    }

    #[test]
    fn pronunciation_policy_survives_borrowing_and_boxing() {
        let permissive = Box::new(dict().with_pronunciation_mode(PronunciationMode::AllowDerived));
        let borrowed = &permissive;
        let word_pronunciations =
            PronouncingDictionary::pronunciations_for(&borrowed, "kettle's").unwrap();
        assert!(matches!(
            word_pronunciations.origin,
            PronunciationOrigin::Derived { .. }
        ));
    }

    #[test]
    fn pronunciations_report_exact_and_normalized_dictionary_origins() {
        let exact = dict().pronunciations_for("hello").unwrap();
        assert!(matches!(exact.pronunciations, Cow::Borrowed(_)));
        assert_eq!(
            exact.origin,
            PronunciationOrigin::Dictionary { normalized: false }
        );

        let normalized = dict().pronunciations_for("cat’s").unwrap();
        assert!(matches!(normalized.pronunciations, Cow::Borrowed(_)));
        assert_eq!(
            normalized.origin,
            PronunciationOrigin::Dictionary { normalized: true }
        );
    }

    #[test]
    fn derives_a_missing_possessive_from_its_base() {
        let dict = dict().with_pronunciation_mode(PronunciationMode::AllowDerived);
        assert!(dict.lookup("kettle's").is_none());

        let pronunciation: Vec<String> =
            dict.pronunciations_for("kettle's").unwrap().pronunciations[0]
                .iter()
                .map(ToString::to_string)
                .collect();
        assert_eq!(pronunciation, ["K", "EH1", "T", "AH0", "L", "Z"]);
    }

    #[test]
    fn derives_possessives_written_with_a_curly_apostrophe() {
        let dict = dict().with_pronunciation_mode(PronunciationMode::AllowDerived);
        assert_eq!(
            dict.pronunciations_for("kettle’s").unwrap(),
            dict.pronunciations_for("kettle's").unwrap()
        );
    }

    #[test]
    fn exact_possessive_entries_take_precedence() {
        let exact = dict().lookup("cat's").unwrap();
        for mode in [
            PronunciationMode::DictionaryOnly,
            PronunciationMode::AllowDerived,
        ] {
            let configured = dict().with_pronunciation_mode(mode);
            for word in ["cat's", "cat’s"] {
                let word_pronunciations = configured.pronunciations_for(word).unwrap();
                assert!(matches!(
                    word_pronunciations.pronunciations,
                    Cow::Borrowed(_)
                ));
                assert_eq!(word_pronunciations.pronunciations.as_ref(), exact);
            }
        }
    }

    #[test]
    fn possessive_of_an_unknown_base_remains_unknown() {
        let dict = dict().with_pronunciation_mode(PronunciationMode::AllowDerived);
        assert!(dict.pronunciations_for("xyzzy's").is_none());
    }

    #[test]
    fn derives_s_forms_from_a_sparse_dictionary() {
        let sparse = CmuDictionary::from_bytes(
            b"DARKEN  D AA1 R K AH0 N\nPASS  P AE1 S\nBABY  B EY1 B IY0\nQUIZ  K W IH1 Z\n",
        )
        .unwrap()
        .with_pronunciation_mode(PronunciationMode::AllowDerived);

        let arpabet = |word: &str| {
            sparse.pronunciations_for(word).unwrap().pronunciations[0]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(arpabet("darkens"), ["D", "AA1", "R", "K", "AH0", "N", "Z"]);
        assert_eq!(arpabet("passes"), ["P", "AE1", "S", "IH0", "Z"]);
        assert_eq!(arpabet("babies"), ["B", "EY1", "B", "IY0", "Z"]);
        assert_eq!(arpabet("quizzes"), ["K", "W", "IH1", "Z", "IH0", "Z"]);
    }

    #[test]
    fn derives_ves_forms_from_a_sparse_dictionary() {
        let sparse = CmuDictionary::from_bytes(
            b"KNIF  K N IH1\nKNIFE  N AY1 F\nLIFE  L AY1 F\nWIFE  W AY1 F\nSHELF  SH EH1 L F\nSCARF  S K AA1 R F\nDWARF  D W AO1 R F\n",
        )
        .unwrap()
        .with_pronunciation_mode(PronunciationMode::AllowDerived);

        let arpabet = |word: &str| {
            sparse.pronunciations_for(word).unwrap().pronunciations[0]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(arpabet("knives"), ["N", "AY1", "V", "Z"]);
        assert_eq!(arpabet("lives"), ["L", "AY1", "V", "Z"]);
        assert_eq!(arpabet("wives"), ["W", "AY1", "V", "Z"]);
        assert_eq!(arpabet("shelves"), ["SH", "EH1", "L", "V", "Z"]);
        assert_eq!(arpabet("scarves"), ["S", "K", "AA1", "R", "V", "Z"]);
        assert_eq!(arpabet("dwarves"), ["D", "W", "AO1", "R", "V", "Z"]);
        assert_eq!(arpabet("scarfs"), ["S", "K", "AA1", "R", "F", "S"]);
        assert_eq!(arpabet("dwarfs"), ["D", "W", "AO1", "R", "F", "S"]);

        assert_eq!(
            sparse.pronunciations_for("knives").unwrap().origin,
            PronunciationOrigin::Derived {
                dictionary_base: "knife".to_string(),
                rules: vec![InflectionRule::Ves],
            }
        );
    }

    #[test]
    fn derives_ly_forms_from_a_sparse_dictionary() {
        let sparse = CmuDictionary::from_bytes(
            b"SURPASSING  S ER0 P AE1 S IH0 NG\nHAPPY  HH AE1 P IY0\nSIMPLE  S IH1 M P AH0 L\nBASIC  B EY1 S IH0 K\nFULL  F UH1 L\nTRUE  T R UW1\nDAY  D EY1\n",
        )
        .unwrap()
        .with_pronunciation_mode(PronunciationMode::AllowDerived);

        let arpabet = |word: &str| {
            sparse.pronunciations_for(word).unwrap().pronunciations[0]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            arpabet("surpassingly"),
            ["S", "ER0", "P", "AE1", "S", "IH0", "NG", "L", "IY0"]
        );
        assert_eq!(arpabet("happily"), ["HH", "AE1", "P", "AH0", "L", "IY0"]);
        assert_eq!(arpabet("simply"), ["S", "IH1", "M", "P", "L", "IY0"]);
        assert_eq!(
            arpabet("basically"),
            ["B", "EY1", "S", "IH0", "K", "L", "IY0"]
        );
        assert_eq!(arpabet("fully"), ["F", "UH1", "L", "IY0"]);
        assert_eq!(arpabet("truly"), ["T", "R", "UW1", "L", "IY0"]);
        assert_eq!(arpabet("daily"), ["D", "EY1", "L", "IY0"]);
    }

    #[test]
    fn derives_ing_and_ed_forms_from_a_sparse_dictionary() {
        let sparse = CmuDictionary::from_bytes(
            b"MAKE  M EY1 K\nRUN  R AH1 N\nDIE  D AY1\nPANIC  P AE1 N IH0 K\nPASS  P AE1 S\nPLAY  P L EY1\nNEED  N IY1 D\nSTUDY  S T AH1 D IY0\nSTOP  S T AA1 P\nLOVE  L AH1 V\n",
        )
        .unwrap()
        .with_pronunciation_mode(PronunciationMode::AllowDerived);

        let arpabet = |word: &str, variant: usize| {
            sparse.pronunciations_for(word).unwrap().pronunciations[variant]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(arpabet("making", 0), ["M", "EY1", "K", "IH0", "NG"]);
        assert_eq!(arpabet("running", 0), ["R", "AH1", "N", "IH0", "NG"]);
        assert_eq!(arpabet("dying", 0), ["D", "AY1", "IH0", "NG"]);
        assert_eq!(
            arpabet("panicking", 0),
            ["P", "AE1", "N", "IH0", "K", "IH0", "NG"]
        );
        assert_eq!(arpabet("passed", 0), ["P", "AE1", "S", "T"]);
        assert_eq!(arpabet("played", 0), ["P", "L", "EY1", "D"]);
        assert_eq!(arpabet("needed", 0), ["N", "IY1", "D", "IH0", "D"]);
        assert_eq!(arpabet("needed", 1), ["N", "IY1", "D", "AH0", "D"]);
        assert_eq!(arpabet("studied", 0), ["S", "T", "AH1", "D", "IY0", "D"]);
        assert_eq!(arpabet("stopped", 0), ["S", "T", "AA1", "P", "T"]);
        assert_eq!(arpabet("loved", 0), ["L", "AH1", "V", "D"]);
    }

    #[test]
    fn derives_er_and_est_forms_from_a_sparse_dictionary() {
        let sparse = CmuDictionary::from_bytes(b"PLAIN  P L EY1 N\nSORE  S AO1 R\n")
            .unwrap()
            .with_pronunciation_mode(PronunciationMode::AllowDerived);

        for word in ["plainer", "plainest", "sorer", "sorest"] {
            assert!(sparse.lookup(word).is_none());
            assert!(sparse.pronunciations_for(word).is_some(), "missing {word}");
        }

        let arpabet = |word: &str, variant: usize| {
            sparse.pronunciations_for(word).unwrap().pronunciations[variant]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(arpabet("plainer", 0), ["P", "L", "EY1", "N", "ER0"]);
        assert_eq!(arpabet("sorer", 0), ["S", "AO1", "R", "ER0"]);
        assert_eq!(
            arpabet("plainest", 0),
            ["P", "L", "EY1", "N", "IH0", "S", "T"]
        );
        assert_eq!(arpabet("sorest", 1), ["S", "AO1", "R", "AH0", "S", "T"]);
    }

    #[test]
    fn composes_pronunciation_derivations() {
        let sparse = CmuDictionary::from_bytes(b"PLAIN  P L EY1 N\n")
            .unwrap()
            .with_pronunciation_mode(PronunciationMode::AllowDerived);
        let word_pronunciations = sparse.pronunciations_for("plainer's").unwrap();
        let arpabet: Vec<String> = word_pronunciations.pronunciations[0]
            .iter()
            .map(ToString::to_string)
            .collect();

        assert_eq!(arpabet, ["P", "L", "EY1", "N", "ER0", "Z"]);
        assert_eq!(
            word_pronunciations.origin,
            PronunciationOrigin::Derived {
                dictionary_base: "plain".to_string(),
                rules: vec![InflectionRule::Er, InflectionRule::ApostropheS],
            }
        );
    }
}
