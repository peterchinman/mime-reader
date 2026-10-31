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

/// A productive suffix: how to recover the written bases a word may have been
/// formed from ([`Self::base_candidates`]) and the sound change applied to a
/// base pronunciation ([`Self::apply`]). Several variants collapse onto one
/// [`InflectionRule`] where a single written suffix is reachable by more than
/// one spelling change.
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
    /// `None` when `word` doesn't carry `suffix`.
    fn new(suffix: InflectionalSuffix, word: &str) -> Option<Self> {
        Some(Self {
            base_candidates: suffix.base_candidates(word)?,
            suffix,
        })
    }

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
    /// Every suffix, in precedence order: `derive_pronunciations` takes the
    /// first one whose base is in the dictionary.
    const PRECEDENCE: [Self; 10] = [
        Self::ApostropheS,
        Self::Er,
        Self::Est,
        Self::S,
        Self::Ves,
        Self::IlyFromY,
        Self::Ly,
        Self::LyFromLe,
        Self::Ing,
        Self::Ed,
    ];

    /// The written bases `word` may have been formed from, or `None` when it
    /// doesn't carry this suffix.
    fn base_candidates(self, word: &str) -> Option<Vec<String>> {
        match self {
            Self::ApostropheS => apostrophe_s_bases(word),
            Self::S => s_bases(word),
            Self::Ves => ves_bases(word),
            Self::Ly => ly_bases(word),
            Self::IlyFromY => ily_from_y_bases(word),
            Self::LyFromLe => ly_from_le_bases(word),
            Self::Ing => ing_bases(word),
            Self::Ed => ed_bases(word),
            Self::Er => er_est_bases(word, "er", "r"),
            Self::Est => er_est_bases(word, "est", "st"),
        }
    }

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

/// The productive rules `word` may have been formed by, in
/// [`InflectionalSuffix::PRECEDENCE`] order. Exact dictionary entries are
/// resolved before any of these rules are consulted.
pub(crate) fn pronunciation_derivations(
    word: &str,
) -> impl Iterator<Item = PronunciationDerivation> {
    InflectionalSuffix::PRECEDENCE
        .into_iter()
        .filter_map(move |suffix| PronunciationDerivation::new(suffix, word))
}

fn apostrophe_s_bases(word: &str) -> Option<Vec<String>> {
    let base = ["'s", "’s"]
        .into_iter()
        .find_map(|suffix| strip_suffix_ignore_ascii_case(word, suffix))
        .filter(|base| !base.is_empty())?;

    Some(vec![base.to_string()])
}

fn strip_suffix_ignore_ascii_case<'a>(word: &'a str, suffix: &str) -> Option<&'a str> {
    let base_len = word.len().checked_sub(suffix.len())?;
    word.get(base_len..)?
        .eq_ignore_ascii_case(suffix)
        .then(|| &word[..base_len])
}

fn s_bases(word: &str) -> Option<Vec<String>> {
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

    Some(base_candidates)
}

fn ves_bases(word: &str) -> Option<Vec<String>> {
    let stem = strip_suffix_ignore_ascii_case(word, "ves").filter(|stem| !stem.is_empty())?;
    let mut base_candidates = Vec::with_capacity(2);

    // shelf -> shelves and knife -> knives. The dictionary decides whether
    // the recovered base ends in f or fe; pronunciation application verifies
    // that the selected base pronunciation actually ends in F.
    push_candidate(&mut base_candidates, with_spelling_suffix(stem, "f", "F"));
    push_candidate(&mut base_candidates, with_spelling_suffix(stem, "fe", "FE"));

    Some(base_candidates)
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

fn ly_stem(word: &str) -> Option<&str> {
    strip_suffix_ignore_ascii_case(word, "ly").filter(|stem| !stem.is_empty())
}

/// happy -> happily, angry -> angrily, and day -> daily. This spelling change
/// can also alter a final unstressed IY0, so it has its own sound
/// transformation rather than sharing the direct -ly rule.
fn ily_from_y_bases(word: &str) -> Option<Vec<String>> {
    restore_final_y(ly_stem(word)?).map(|y_base| vec![y_base])
}

fn ly_bases(word: &str) -> Option<Vec<String>> {
    let stem = ly_stem(word)?;
    let mut base_candidates = Vec::with_capacity(4);

    // basic -> basically and logic -> logically insert written -al-, but the
    // pronunciation is still the ordinary L IY0 ending.
    if let Some(ic_base) = strip_suffix_ignore_ascii_case(stem, "al")
        && strip_suffix_ignore_ascii_case(ic_base, "ic").is_some()
    {
        push_candidate(&mut base_candidates, ic_base.to_string());
    }
    push_candidate(&mut base_candidates, stem.to_string());
    push_candidate(&mut base_candidates, restore_final_e(stem));
    if let Some(doubled_l_base) = double_final_l(stem) {
        push_candidate(&mut base_candidates, doubled_l_base);
    }

    Some(base_candidates)
}

/// simple -> simply and terrible -> terribly remove the written e and the weak
/// vowel before the base's final L.
fn ly_from_le_bases(word: &str) -> Option<Vec<String>> {
    Some(vec![restore_final_le(ly_stem(word)?)])
}

fn ing_bases(word: &str) -> Option<Vec<String>> {
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

    Some(base_candidates)
}

fn ed_bases(word: &str) -> Option<Vec<String>> {
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

    Some(base_candidates)
}

/// Shared by -er and -est. `silent_e_suffix` is what remains of the written
/// suffix after a base's final e (sore -> sorer, sorest).
fn er_est_bases(word: &str, written_suffix: &str, silent_e_suffix: &str) -> Option<Vec<String>> {
    let stem = strip_suffix_ignore_ascii_case(word, written_suffix)?;
    let silent_e_base = strip_suffix_ignore_ascii_case(word, silent_e_suffix)?;
    let mut base_candidates = Vec::with_capacity(4);

    if let Some(y_base) = restore_final_y(stem) {
        push_candidate(&mut base_candidates, y_base);
    }
    if let Some(undoubled_base) = undouble_final_letter(stem) {
        push_candidate(&mut base_candidates, undoubled_base);
    }
    push_candidate(&mut base_candidates, stem.to_string());
    push_candidate(&mut base_candidates, silent_e_base.to_string());

    (!base_candidates.is_empty()).then_some(base_candidates)
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

    fn bases(suffix: InflectionalSuffix, word: &str) -> Vec<String> {
        suffix.base_candidates(word).unwrap()
    }

    fn derive(suffix: InflectionalSuffix, word: &str) -> PronunciationDerivation {
        PronunciationDerivation::new(suffix, word).unwrap()
    }

    fn ly_derivation(word: &str, expected_base: &str) -> PronunciationDerivation {
        [
            InflectionalSuffix::IlyFromY,
            InflectionalSuffix::Ly,
            InflectionalSuffix::LyFromLe,
        ]
        .into_iter()
        .filter_map(|suffix| PronunciationDerivation::new(suffix, word))
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
            assert_eq!(
                bases(InflectionalSuffix::ApostropheS, word),
                [expected_base]
            );
        }
    }

    #[test]
    fn recognizes_er_est_and_silent_e_bases() {
        assert_eq!(
            bases(InflectionalSuffix::Est, "plainest"),
            ["plain", "plaine"]
        );
        assert_eq!(
            bases(InflectionalSuffix::Er, "plainer"),
            ["plain", "plaine"]
        );
        assert_eq!(bases(InflectionalSuffix::Est, "sorest"), ["sor", "sore"]);
        assert_eq!(bases(InflectionalSuffix::Er, "sorer"), ["sor", "sore"]);
    }

    #[test]
    fn recognizes_s_spelling_changes() {
        assert_eq!(bases(InflectionalSuffix::S, "darkens"), ["darken"]);
        assert!(
            bases(InflectionalSuffix::S, "passes")
                .iter()
                .any(|base| base == "pass")
        );
        assert_eq!(bases(InflectionalSuffix::S, "babies")[0], "baby");
        assert!(
            bases(InflectionalSuffix::S, "quizzes")
                .iter()
                .any(|base| base == "quiz")
        );
        assert_eq!(bases(InflectionalSuffix::S, "photos"), ["photo"]);
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
            let candidates = bases(InflectionalSuffix::Ves, inflected);
            assert!(
                includes_base(&candidates, expected_base),
                "{inflected} should include {expected_base}; got {candidates:?}"
            );
        }

        let derivation = derive(InflectionalSuffix::Ves, "knives");
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
        assert_eq!(bases(InflectionalSuffix::Ing, "darkening")[0], "darken");
        assert!(includes_base(
            &bases(InflectionalSuffix::Ing, "making"),
            "make"
        ));
        assert!(includes_base(
            &bases(InflectionalSuffix::Ing, "running"),
            "run"
        ));
        assert_eq!(bases(InflectionalSuffix::Ing, "dying")[0], "die");
        assert!(includes_base(
            &bases(InflectionalSuffix::Ing, "panicking"),
            "panic"
        ));
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
            let candidates = bases(InflectionalSuffix::Ed, inflected);
            assert!(
                includes_base(&candidates, expected_base),
                "{inflected} should include {expected_base}; got {candidates:?}"
            );
        }
    }

    #[test]
    fn recognizes_y_changes_and_doubled_consonants() {
        assert_eq!(bases(InflectionalSuffix::Er, "happier")[0], "happy");
        assert_eq!(bases(InflectionalSuffix::Est, "happiest")[0], "happy");
        assert_eq!(bases(InflectionalSuffix::Er, "bigger")[0], "big");
        assert_eq!(bases(InflectionalSuffix::Est, "biggest")[0], "big");
    }

    #[test]
    fn er_suffix_adds_unstressed_er() {
        let derivation = derive(InflectionalSuffix::Er, "plainer");
        let pronunciations = derivation.apply(&[phonemes(&["P", "L", "EY1", "N"])]);

        assert_eq!(
            as_arpabet(&pronunciations[0]),
            ["P", "L", "EY1", "N", "ER0"]
        );
    }

    #[test]
    fn est_suffix_includes_both_cmudict_weak_vowels() {
        let derivation = derive(InflectionalSuffix::Est, "plainest");
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
