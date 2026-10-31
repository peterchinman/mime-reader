use crate::{
    Stress,
    phoneme::{ConsonantPhoneme, Phoneme, Vowel, VowelPhoneme},
};

/// A productive spelling and pronunciation rule used to resolve a word that
/// does not have an exact dictionary entry.
///
/// Variants are named for the written suffix rather than for a grammatical
/// category, because the rules match spelling alone. [`Self::Er`] fires on
/// *baker* as readily as on *bolder*, and [`Self::Ly`] on *friendly* as
/// readily as on *boldly*; each yields the right pronunciation without the
/// word being a comparative or an adverb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InflectionRule {
    /// `'s` or `’s`: possessives and contractions alike.
    ApostropheS,
    /// `-s` or `-es`: noun plurals and third-person singular verbs.
    S,
    /// `-ves` over a base in f or fe: *shelf -> shelves*, *knife -> knives*.
    Ves,
    /// `-ly`, however the base's spelling changes to accommodate it.
    Ly,
    /// `-ing`: present participles and gerunds.
    Ing,
    /// `-ed` or `-d`: past tenses and past participles.
    Ed,
    /// `-er`: comparatives and agentive nouns.
    Er,
    /// `-est`.
    Est,
}

/// The sound change applied to a base pronunciation. Several variants collapse
/// onto one [`InflectionRule`] where a single written suffix is reachable by
/// more than one spelling change.
#[derive(Clone, Copy)]
enum InflectionalSuffix {
    ApostropheS,
    S,
    Ves,
    Ly,
    IlyFromY,
    LyFromLe,
    Ing,
    Ed,
    Er,
    Est,
}

/// A productive pronunciation rule and the possible written bases from which
/// its input word may have been formed.
pub(crate) struct PronunciationDerivation {
    base_candidates: Vec<String>,
    suffix: InflectionalSuffix,
}

impl PronunciationDerivation {
    pub(crate) fn base_candidates(&self) -> impl Iterator<Item = &str> {
        self.base_candidates.iter().map(String::as_str)
    }

    pub(crate) fn apply(&self, base_pronunciations: &[Box<[Phoneme]>]) -> Vec<Box<[Phoneme]>> {
        base_pronunciations
            .iter()
            .flat_map(|pronunciation| self.suffix.apply(pronunciation))
            .collect()
    }

    pub(crate) fn rule(&self) -> InflectionRule {
        self.suffix.rule()
    }
}

impl InflectionalSuffix {
    fn rule(self) -> InflectionRule {
        match self {
            Self::ApostropheS => InflectionRule::ApostropheS,
            Self::S => InflectionRule::S,
            Self::Ves => InflectionRule::Ves,
            Self::Ly | Self::IlyFromY | Self::LyFromLe => InflectionRule::Ly,
            Self::Ing => InflectionRule::Ing,
            Self::Ed => InflectionRule::Ed,
            Self::Er => InflectionRule::Er,
            Self::Est => InflectionRule::Est,
        }
    }

    fn apply(self, base: &[Phoneme]) -> Vec<Box<[Phoneme]>> {
        match self {
            Self::ApostropheS | Self::S => vec![add_s_suffix(base)],
            Self::Ves => add_ves_suffix(base).into_iter().collect(),
            Self::Ly => vec![add_ly_suffix(base)],
            Self::IlyFromY => vec![add_ily_suffix(base)],
            Self::LyFromLe => vec![add_le_to_ly_suffix(base)],
            Self::Ing => vec![append_suffix(
                base,
                [
                    vowel_phoneme(VowelPhoneme::IH),
                    consonant_phoneme(ConsonantPhoneme::NG),
                ],
            )],
            Self::Ed => add_ed_suffix(base),
            Self::Er => vec![append_suffix(base, [vowel_phoneme(VowelPhoneme::ER)])],
            // CMUdict represents the weak vowel in -est with both IH0 and
            // AH0. Keep both variants so rhyme analysis can choose while
            // meter sees the same unstressed syllable in either case.
            Self::Est => vec![
                append_suffix(
                    base,
                    [
                        vowel_phoneme(VowelPhoneme::IH),
                        consonant_phoneme(ConsonantPhoneme::S),
                        consonant_phoneme(ConsonantPhoneme::T),
                    ],
                ),
                append_suffix(
                    base,
                    [
                        vowel_phoneme(VowelPhoneme::AH),
                        consonant_phoneme(ConsonantPhoneme::S),
                        consonant_phoneme(ConsonantPhoneme::T),
                    ],
                ),
            ],
        }
    }
}

/// Productive inflection rules, in precedence order: `derive_pronunciations`
/// takes the first one whose base is in the dictionary. Exact dictionary
/// entries are resolved before any of these rules are consulted.
pub(crate) fn pronunciation_derivations(
    word: &str,
) -> impl Iterator<Item = PronunciationDerivation> {
    let mut derivations = Vec::new();
    derivations.extend(apostrophe_s_derivation(word));
    derivations.extend(er_est_derivation(word));
    derivations.extend(s_derivation(word));
    derivations.extend(ves_derivation(word));
    derivations.extend(ly_derivations(word));
    derivations.extend(ing_derivation(word));
    derivations.extend(ed_derivation(word));
    derivations.into_iter()
}

fn apostrophe_s_derivation(word: &str) -> Option<PronunciationDerivation> {
    let base = ["'s", "’s"]
        .into_iter()
        .find_map(|suffix| strip_suffix_ignore_ascii_case(word, suffix))
        .filter(|base| !base.is_empty())?;

    Some(PronunciationDerivation {
        base_candidates: vec![base.to_string()],
        suffix: InflectionalSuffix::ApostropheS,
    })
}

fn strip_suffix_ignore_ascii_case<'a>(word: &'a str, suffix: &str) -> Option<&'a str> {
    let base_len = word.len().checked_sub(suffix.len())?;
    word.get(base_len..)?
        .eq_ignore_ascii_case(suffix)
        .then(|| &word[..base_len])
}

fn s_derivation(word: &str) -> Option<PronunciationDerivation> {
    let s_base = strip_suffix_ignore_ascii_case(word, "s").filter(|base| !base.is_empty())?;
    let es_base = strip_suffix_ignore_ascii_case(word, "es").filter(|base| !base.is_empty());
    let mut base_candidates = Vec::with_capacity(4);

    if let Some(es_base) = es_base
        && let Some(y_base) = restore_final_y_after_consonant(es_base)
    {
        push_candidate(&mut base_candidates, y_base);
    }

    // Trying the simple -s spelling first preserves bases that already end in
    // e (house -> houses, toe -> toes). If that is not a dictionary word, the
    // -es candidates cover pass -> passes, fox -> foxes, and similar forms.
    push_candidate(&mut base_candidates, s_base.to_string());
    if let Some(es_base) = es_base {
        push_candidate(&mut base_candidates, es_base.to_string());
        if let Some(undoubled_base) = undouble_final_letter(es_base) {
            push_candidate(&mut base_candidates, undoubled_base);
        }
    }

    Some(PronunciationDerivation {
        base_candidates,
        suffix: InflectionalSuffix::S,
    })
}

fn ves_derivation(word: &str) -> Option<PronunciationDerivation> {
    let stem = strip_suffix_ignore_ascii_case(word, "ves").filter(|stem| !stem.is_empty())?;
    let mut base_candidates = Vec::with_capacity(2);

    // shelf -> shelves and knife -> knives. The dictionary decides whether
    // the recovered base ends in f or fe; pronunciation application verifies
    // that the selected base pronunciation actually ends in F.
    push_candidate(&mut base_candidates, with_spelling_suffix(stem, "f", "F"));
    push_candidate(&mut base_candidates, with_spelling_suffix(stem, "fe", "FE"));

    Some(PronunciationDerivation {
        base_candidates,
        suffix: InflectionalSuffix::Ves,
    })
}

fn with_spelling_suffix(stem: &str, lowercase: &str, uppercase: &str) -> String {
    let mut restored = String::with_capacity(stem.len() + lowercase.len());
    restored.push_str(stem);
    restored.push_str(
        if stem
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_uppercase())
        {
            uppercase
        } else {
            lowercase
        },
    );
    restored
}

fn ly_derivations(word: &str) -> Vec<PronunciationDerivation> {
    let Some(stem) = strip_suffix_ignore_ascii_case(word, "ly").filter(|stem| !stem.is_empty())
    else {
        return Vec::new();
    };
    let mut derivations = Vec::with_capacity(3);

    // happy -> happily, angry -> angrily, and day -> daily. This spelling
    // change can also alter a final unstressed IY0, so it has its own sound
    // transformation rather than sharing the direct -ly rule.
    if let Some(y_base) = restore_final_y(stem) {
        derivations.push(PronunciationDerivation {
            base_candidates: vec![y_base],
            suffix: InflectionalSuffix::IlyFromY,
        });
    }

    let mut ly_bases = Vec::with_capacity(4);
    // basic -> basically and logic -> logically insert written -al-, but the
    // pronunciation is still the ordinary L IY0 ending.
    if let Some(ic_base) = strip_suffix_ignore_ascii_case(stem, "al")
        && strip_suffix_ignore_ascii_case(ic_base, "ic").is_some()
    {
        push_candidate(&mut ly_bases, ic_base.to_string());
    }
    push_candidate(&mut ly_bases, stem.to_string());
    push_candidate(&mut ly_bases, restore_final_e(stem));
    if let Some(doubled_l_base) = double_final_l(stem) {
        push_candidate(&mut ly_bases, doubled_l_base);
    }
    derivations.push(PronunciationDerivation {
        base_candidates: ly_bases,
        suffix: InflectionalSuffix::Ly,
    });

    // simple -> simply and terrible -> terribly remove the written e and the
    // weak vowel before the base's final L.
    derivations.push(PronunciationDerivation {
        base_candidates: vec![restore_final_le(stem)],
        suffix: InflectionalSuffix::LyFromLe,
    });

    derivations
}

fn ing_derivation(word: &str) -> Option<PronunciationDerivation> {
    let stem = strip_suffix_ignore_ascii_case(word, "ing").filter(|stem| !stem.is_empty())?;
    let mut base_candidates = Vec::with_capacity(5);

    if let Some(ie_base) = restore_final_ie(stem) {
        push_candidate(&mut base_candidates, ie_base);
    }
    push_candidate(&mut base_candidates, stem.to_string());
    if let Some(undoubled_base) = undouble_final_letter(stem) {
        push_candidate(&mut base_candidates, undoubled_base);
    }
    if let Some(c_base) = remove_final_k_after_c(stem) {
        push_candidate(&mut base_candidates, c_base);
    }
    push_candidate(&mut base_candidates, restore_final_e(stem));

    Some(PronunciationDerivation {
        base_candidates,
        suffix: InflectionalSuffix::Ing,
    })
}

fn ed_derivation(word: &str) -> Option<PronunciationDerivation> {
    let d_base = strip_suffix_ignore_ascii_case(word, "d").filter(|base| !base.is_empty())?;
    let ed_base = strip_suffix_ignore_ascii_case(word, "ed").filter(|base| !base.is_empty());
    let mut base_candidates = Vec::with_capacity(5);

    // Prefer the final-e spelling (love -> loved) when it exists. The other
    // candidates cover study -> studied, walk -> walked, stop -> stopped, and
    // panic -> panicked.
    push_candidate(&mut base_candidates, d_base.to_string());
    if let Some(ed_base) = ed_base {
        if let Some(y_base) = restore_final_y_after_consonant(ed_base) {
            push_candidate(&mut base_candidates, y_base);
        }
        push_candidate(&mut base_candidates, ed_base.to_string());
        if let Some(undoubled_base) = undouble_final_letter(ed_base) {
            push_candidate(&mut base_candidates, undoubled_base);
        }
        if let Some(c_base) = remove_final_k_after_c(ed_base) {
            push_candidate(&mut base_candidates, c_base);
        }
    }

    Some(PronunciationDerivation {
        base_candidates,
        suffix: InflectionalSuffix::Ed,
    })
}

fn er_est_derivation(word: &str) -> Option<PronunciationDerivation> {
    let lowercase = word.to_ascii_lowercase();
    let (written_suffix, silent_e_suffix, suffix) = if lowercase.ends_with("est") {
        ("est", "st", InflectionalSuffix::Est)
    } else if lowercase.ends_with("er") {
        ("er", "r", InflectionalSuffix::Er)
    } else {
        return None;
    };

    let stem = &word[..word.len() - written_suffix.len()];
    let silent_e_base = &word[..word.len() - silent_e_suffix.len()];
    let mut base_candidates = Vec::with_capacity(4);

    if let Some(y_base) = restore_final_y(stem) {
        push_candidate(&mut base_candidates, y_base);
    }
    if let Some(undoubled_base) = undouble_final_letter(stem) {
        push_candidate(&mut base_candidates, undoubled_base);
    }
    push_candidate(&mut base_candidates, stem.to_string());
    push_candidate(&mut base_candidates, silent_e_base.to_string());

    (!base_candidates.is_empty()).then_some(PronunciationDerivation {
        base_candidates,
        suffix,
    })
}

fn restore_final_y(stem: &str) -> Option<String> {
    let base = stem.strip_suffix('i').or_else(|| stem.strip_suffix('I'))?;
    let mut restored = String::with_capacity(stem.len());
    restored.push_str(base);
    restored.push(if stem.ends_with('I') { 'Y' } else { 'y' });
    Some(restored)
}

fn restore_final_y_after_consonant(stem: &str) -> Option<String> {
    let base = stem.strip_suffix('i').or_else(|| stem.strip_suffix('I'))?;
    let preceding = base.chars().next_back()?;
    let preceded_by_consonant = preceding.is_ascii_alphabetic()
        && !matches!(preceding.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u');
    if !preceded_by_consonant {
        return None;
    }
    restore_final_y(stem)
}

fn restore_final_ie(stem: &str) -> Option<String> {
    let base = stem.strip_suffix('y').or_else(|| stem.strip_suffix('Y'))?;
    let mut restored = String::with_capacity(stem.len() + 1);
    restored.push_str(base);
    restored.push_str(if stem.ends_with('Y') { "IE" } else { "ie" });
    Some(restored)
}

fn restore_final_e(stem: &str) -> String {
    let mut restored = String::with_capacity(stem.len() + 1);
    restored.push_str(stem);
    restored.push(
        if stem
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_uppercase())
        {
            'E'
        } else {
            'e'
        },
    );
    restored
}

fn restore_final_le(stem: &str) -> String {
    let mut restored = String::with_capacity(stem.len() + 2);
    restored.push_str(stem);
    restored.push_str(
        if stem
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_uppercase())
        {
            "LE"
        } else {
            "le"
        },
    );
    restored
}

fn double_final_l(stem: &str) -> Option<String> {
    let final_l = stem
        .chars()
        .next_back()
        .filter(|character| character.eq_ignore_ascii_case(&'l'))?;
    let mut restored = String::with_capacity(stem.len() + final_l.len_utf8());
    restored.push_str(stem);
    restored.push(final_l);
    Some(restored)
}

fn remove_final_k_after_c(stem: &str) -> Option<String> {
    let base = stem.strip_suffix('k').or_else(|| stem.strip_suffix('K'))?;
    base.chars()
        .next_back()?
        .eq_ignore_ascii_case(&'c')
        .then(|| base.to_string())
}

fn undouble_final_letter(stem: &str) -> Option<String> {
    let mut characters = stem.char_indices().rev();
    let (last_index, last) = characters.next()?;
    let (_, previous) = characters.next()?;
    last.eq_ignore_ascii_case(&previous)
        .then(|| stem[..last_index].to_string())
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty()
        && !candidates
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&candidate))
    {
        candidates.push(candidate);
    }
}

fn add_s_suffix(base: &[Phoneme]) -> Box<[Phoneme]> {
    let Some(final_phoneme) = base.last() else {
        return Box::default();
    };

    let suffix = match final_phoneme {
        Phoneme::Consonant(consonant) if consonant.sibilant => vec![
            vowel_phoneme(VowelPhoneme::IH),
            consonant_phoneme(ConsonantPhoneme::Z),
        ],
        Phoneme::Consonant(consonant) if !consonant.voiced => {
            vec![consonant_phoneme(ConsonantPhoneme::S)]
        }
        Phoneme::Consonant(_) | Phoneme::Vowel(_) => {
            vec![consonant_phoneme(ConsonantPhoneme::Z)]
        }
    };

    let mut inflected = Vec::with_capacity(base.len() + suffix.len());
    inflected.extend_from_slice(base);
    inflected.extend(suffix);
    inflected.into_boxed_slice()
}

fn add_ves_suffix(base: &[Phoneme]) -> Option<Box<[Phoneme]>> {
    matches!(
        base.last(),
        Some(Phoneme::Consonant(consonant)) if consonant.phoneme == ConsonantPhoneme::F
    )
    .then(|| {
        append_suffix(
            &base[..base.len() - 1],
            [
                consonant_phoneme(ConsonantPhoneme::V),
                consonant_phoneme(ConsonantPhoneme::Z),
            ],
        )
    })
}

fn add_ly_suffix(base: &[Phoneme]) -> Box<[Phoneme]> {
    if matches!(
        base.last(),
        Some(Phoneme::Consonant(consonant)) if consonant.phoneme == ConsonantPhoneme::L
    ) {
        append_suffix(base, [vowel_phoneme(VowelPhoneme::IY)])
    } else {
        append_suffix(
            base,
            [
                consonant_phoneme(ConsonantPhoneme::L),
                vowel_phoneme(VowelPhoneme::IY),
            ],
        )
    }
}

fn add_ily_suffix(base: &[Phoneme]) -> Box<[Phoneme]> {
    if matches!(
        base.last(),
        Some(Phoneme::Vowel(vowel))
            if vowel.phoneme == VowelPhoneme::IY && vowel.stress == Stress::Unstressed
    ) {
        append_suffix(
            &base[..base.len() - 1],
            [
                vowel_phoneme(VowelPhoneme::AH),
                consonant_phoneme(ConsonantPhoneme::L),
                vowel_phoneme(VowelPhoneme::IY),
            ],
        )
    } else {
        add_ly_suffix(base)
    }
}

fn add_le_to_ly_suffix(base: &[Phoneme]) -> Box<[Phoneme]> {
    if base.len() >= 2
        && matches!(
            &base[base.len() - 2..],
            [Phoneme::Vowel(vowel), Phoneme::Consonant(consonant)]
                if vowel.stress == Stress::Unstressed
                    && consonant.phoneme == ConsonantPhoneme::L
        )
    {
        append_suffix(
            &base[..base.len() - 2],
            [
                consonant_phoneme(ConsonantPhoneme::L),
                vowel_phoneme(VowelPhoneme::IY),
            ],
        )
    } else {
        add_ly_suffix(base)
    }
}

fn add_ed_suffix(base: &[Phoneme]) -> Vec<Box<[Phoneme]>> {
    let Some(final_phoneme) = base.last() else {
        return vec![Box::default()];
    };

    match final_phoneme {
        // CMUdict represents the weak vowel in syllabic -ed with both IH0 and
        // AH0. Both have the same meter, while rhyme analysis may distinguish
        // them.
        Phoneme::Consonant(consonant)
            if matches!(consonant.phoneme, ConsonantPhoneme::T | ConsonantPhoneme::D) =>
        {
            vec![
                append_suffix(
                    base,
                    [
                        vowel_phoneme(VowelPhoneme::IH),
                        consonant_phoneme(ConsonantPhoneme::D),
                    ],
                ),
                append_suffix(
                    base,
                    [
                        vowel_phoneme(VowelPhoneme::AH),
                        consonant_phoneme(ConsonantPhoneme::D),
                    ],
                ),
            ]
        }
        Phoneme::Consonant(consonant) if !consonant.voiced => {
            vec![append_suffix(
                base,
                [consonant_phoneme(ConsonantPhoneme::T)],
            )]
        }
        Phoneme::Consonant(_) | Phoneme::Vowel(_) => vec![append_suffix(
            base,
            [consonant_phoneme(ConsonantPhoneme::D)],
        )],
    }
}

fn append_suffix<const N: usize>(base: &[Phoneme], suffix: [Phoneme; N]) -> Box<[Phoneme]> {
    let mut pronunciation = Vec::with_capacity(base.len() + suffix.len());
    pronunciation.extend_from_slice(base);
    pronunciation.extend(suffix);
    pronunciation.into_boxed_slice()
}

fn vowel_phoneme(phoneme: VowelPhoneme) -> Phoneme {
    Phoneme::Vowel(Vowel {
        phoneme,
        stress: Stress::Unstressed,
    })
}

fn consonant_phoneme(phoneme: ConsonantPhoneme) -> Phoneme {
    Phoneme::Consonant(phoneme.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phonemes(values: &[&str]) -> Box<[Phoneme]> {
        values
            .iter()
            .map(|value| value.parse().unwrap())
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn as_arpabet(pronunciation: &[Phoneme]) -> Vec<String> {
        pronunciation.iter().map(ToString::to_string).collect()
    }

    fn er_est_bases(word: &str) -> Vec<String> {
        er_est_derivation(word).unwrap().base_candidates
    }

    fn s_bases(word: &str) -> Vec<String> {
        s_derivation(word).unwrap().base_candidates
    }

    fn ves_bases(word: &str) -> Vec<String> {
        ves_derivation(word).unwrap().base_candidates
    }

    fn ing_bases(word: &str) -> Vec<String> {
        ing_derivation(word).unwrap().base_candidates
    }

    fn ed_bases(word: &str) -> Vec<String> {
        ed_derivation(word).unwrap().base_candidates
    }

    fn ly_derivation(word: &str, expected_base: &str) -> PronunciationDerivation {
        ly_derivations(word)
            .into_iter()
            .find(|derivation| {
                derivation
                    .base_candidates()
                    .any(|candidate| candidate == expected_base)
            })
            .unwrap()
    }

    fn includes_base(candidates: &[String], expected: &str) -> bool {
        candidates.iter().any(|candidate| candidate == expected)
    }

    #[test]
    fn recognizes_apostrophe_s_suffixes_without_changing_base_case() {
        for (word, expected_base) in [
            ("kettle's", "kettle"),
            ("kettle'S", "kettle"),
            ("NASA’s", "NASA"),
            ("NASA’S", "NASA"),
        ] {
            let derivation = apostrophe_s_derivation(word).unwrap();
            assert_eq!(derivation.base_candidates, [expected_base]);
        }
    }

    #[test]
    fn recognizes_er_est_and_silent_e_bases() {
        assert_eq!(er_est_bases("plainest"), ["plain", "plaine"]);
        assert_eq!(er_est_bases("plainer"), ["plain", "plaine"]);
        assert_eq!(er_est_bases("sorest"), ["sor", "sore"]);
        assert_eq!(er_est_bases("sorer"), ["sor", "sore"]);
    }

    #[test]
    fn recognizes_s_spelling_changes() {
        assert_eq!(s_bases("darkens"), ["darken"]);
        assert!(s_bases("passes").iter().any(|base| base == "pass"));
        assert_eq!(s_bases("babies")[0], "baby");
        assert!(s_bases("quizzes").iter().any(|base| base == "quiz"));
        assert_eq!(s_bases("photos"), ["photo"]);
    }

    #[test]
    fn ves_restores_f_bases_and_voices_the_final_sound() {
        for (inflected, expected_base) in [
            ("knives", "knife"),
            ("lives", "life"),
            ("wives", "wife"),
            ("shelves", "shelf"),
            ("scarves", "scarf"),
            ("dwarves", "dwarf"),
        ] {
            let candidates = ves_bases(inflected);
            assert!(
                includes_base(&candidates, expected_base),
                "{inflected} should include {expected_base}; got {candidates:?}"
            );
        }

        let derivation = ves_derivation("knives").unwrap();
        let pronunciations = derivation.apply(&[phonemes(&["N", "AY1", "F"])]);
        assert_eq!(as_arpabet(&pronunciations[0]), ["N", "AY1", "V", "Z"]);

        assert!(
            derivation
                .apply(&[phonemes(&["K", "EY1", "F", "EY0"])])
                .is_empty()
        );
    }

    #[test]
    fn ly_preserves_the_written_base_and_adds_an_unstressed_syllable() {
        let derivation = ly_derivation("surpassingly", "surpassing");

        let pronunciations =
            derivation.apply(&[phonemes(&["S", "ER0", "P", "AE1", "S", "IH0", "NG"])]);
        assert_eq!(
            as_arpabet(&pronunciations[0]),
            ["S", "ER0", "P", "AE1", "S", "IH0", "NG", "L", "IY0"]
        );
    }

    #[test]
    fn ly_handles_common_spelling_and_sound_changes() {
        let cases: [(&str, &str, &[&str], &[&str]); 6] = [
            (
                "happily",
                "happy",
                &["HH", "AE1", "P", "IY0"],
                &["HH", "AE1", "P", "AH0", "L", "IY0"],
            ),
            (
                "simply",
                "simple",
                &["S", "IH1", "M", "P", "AH0", "L"],
                &["S", "IH1", "M", "P", "L", "IY0"],
            ),
            (
                "basically",
                "basic",
                &["B", "EY1", "S", "IH0", "K"],
                &["B", "EY1", "S", "IH0", "K", "L", "IY0"],
            ),
            (
                "fully",
                "full",
                &["F", "UH1", "L"],
                &["F", "UH1", "L", "IY0"],
            ),
            (
                "truly",
                "true",
                &["T", "R", "UW1"],
                &["T", "R", "UW1", "L", "IY0"],
            ),
            ("daily", "day", &["D", "EY1"], &["D", "EY1", "L", "IY0"]),
        ];
        for (word, expected_base, base, expected) in cases {
            let pronunciations = ly_derivation(word, expected_base).apply(&[phonemes(base)]);
            assert_eq!(as_arpabet(&pronunciations[0]), expected);
        }
    }

    #[test]
    fn recognizes_ing_spelling_changes() {
        assert_eq!(ing_bases("darkening")[0], "darken");
        assert!(includes_base(&ing_bases("making"), "make"));
        assert!(includes_base(&ing_bases("running"), "run"));
        assert_eq!(ing_bases("dying")[0], "die");
        assert!(includes_base(&ing_bases("panicking"), "panic"));
    }

    #[test]
    fn recognizes_ed_spelling_changes() {
        let cases = [
            // A base ending in e takes only d.
            ("lived", "live"),
            ("loved", "love"),
            ("smiled", "smile"),
            ("danced", "dance"),
            // A stressed consonant-vowel-consonant ending doubles its final
            // consonant.
            ("stopped", "stop"),
            ("admitted", "admit"),
            ("planned", "plan"),
            ("referred", "refer"),
            ("committed", "commit"),
            // An unstressed final syllable does not double its consonant.
            ("happened", "happen"),
            ("entered", "enter"),
            ("offered", "offer"),
            ("suffered", "suffer"),
            // Final w, x, and y remain single.
            ("fixed", "fix"),
            ("enjoyed", "enjoy"),
            ("snowed", "snow"),
            // Both UK and US final-l spellings resolve to the same base.
            ("travelled", "travel"),
            ("traveled", "travel"),
            ("marvelled", "marvel"),
            ("marveled", "marvel"),
            // Additional spelling transformations supported by this module.
            ("darkened", "darken"),
            ("studied", "study"),
            ("panicked", "panic"),
        ];

        for (inflected, expected_base) in cases {
            let candidates = ed_bases(inflected);
            assert!(
                includes_base(&candidates, expected_base),
                "{inflected} should include {expected_base}; got {candidates:?}"
            );
        }
    }

    #[test]
    fn recognizes_y_changes_and_doubled_consonants() {
        assert_eq!(er_est_bases("happier")[0], "happy");
        assert_eq!(er_est_bases("happiest")[0], "happy");
        assert_eq!(er_est_bases("bigger")[0], "big");
        assert_eq!(er_est_bases("biggest")[0], "big");
    }

    #[test]
    fn er_suffix_adds_unstressed_er() {
        let derivation = er_est_derivation("plainer").unwrap();
        let pronunciations = derivation.apply(&[phonemes(&["P", "L", "EY1", "N"])]);

        assert_eq!(
            as_arpabet(&pronunciations[0]),
            ["P", "L", "EY1", "N", "ER0"]
        );
    }

    #[test]
    fn est_suffix_includes_both_cmudict_weak_vowels() {
        let derivation = er_est_derivation("plainest").unwrap();
        let pronunciations = derivation.apply(&[phonemes(&["P", "L", "EY1", "N"])]);

        assert_eq!(
            as_arpabet(&pronunciations[0]),
            ["P", "L", "EY1", "N", "IH0", "S", "T"]
        );
        assert_eq!(
            as_arpabet(&pronunciations[1]),
            ["P", "L", "EY1", "N", "AH0", "S", "T"]
        );
    }

    #[test]
    fn s_suffix_uses_s_after_a_voiceless_non_sibilant() {
        assert_eq!(
            as_arpabet(&add_s_suffix(&phonemes(&["K", "AE1", "T"]))),
            ["K", "AE1", "T", "S"]
        );
    }

    #[test]
    fn s_suffix_uses_z_after_a_voiced_sound() {
        assert_eq!(
            as_arpabet(&add_s_suffix(&phonemes(&["K", "EH1", "T", "AH0", "L"]))),
            ["K", "EH1", "T", "AH0", "L", "Z"]
        );
    }

    #[test]
    fn s_suffix_uses_an_unstressed_syllable_after_a_sibilant() {
        assert_eq!(
            as_arpabet(&add_s_suffix(&phonemes(&["B", "AH1", "S"]))),
            ["B", "AH1", "S", "IH0", "Z"]
        );
    }
}
