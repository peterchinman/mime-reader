use std::collections::{HashMap, HashSet};

use crate::{
    Line, OwnedAlignmentStep, RhymeDistanceScorer,
    consonant_distance::ConsonantDistanceScorer,
    error::{ParseRhymeError, RhymeCheckError, UnknownWordError},
    line::{WordData, WordEntry},
    phoneme::{Phoneme, RhymingPart, get_last_n_syllables, get_rhyming_part},
    vowel_distance::VowelDistanceScorer,
};

pub struct RhymeScheme {
    scheme: Vec<SchemeRole>,
    comparison_config: RhymeComparisonConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RhymeNormalization {
    None,
    PerSyllable,
    #[default]
    PerMeanPhoneme,
}

impl RhymeNormalization {
    fn denominator(
        self,
        syllable_count: usize,
        leader_phoneme_count: usize,
        target_phoneme_count: usize,
    ) -> f32 {
        match self {
            Self::None => 1.0,
            Self::PerSyllable => syllable_count as f32,
            Self::PerMeanPhoneme => (leader_phoneme_count + target_phoneme_count) as f32 / 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RhymeComparisonConfig {
    pub threshold: f32,
    pub normalization: RhymeNormalization,
}

impl Default for RhymeComparisonConfig {
    fn default() -> Self {
        Self {
            threshold: 10.0,
            normalization: RhymeNormalization::default(),
        }
    }
}

/// A role within the stored rhyme scheme pattern.
///
/// Positions here are scheme-relative: for example, in `ABAB`, the third
/// position is a follower whose `leader_position` is `0`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SchemeRole {
    /// The first occurrence of a rhyme label within the scheme pattern.
    Leader,
    /// A scheme position that imposes no rhyme constraint, represented by `_`.
    Unconstrained,
    /// A repeated rhyme label that should rhyme with an earlier scheme position.
    Follower {
        /// The index within the scheme pattern of the label's first occurrence.
        leader_position: usize,
    },
}

/// A role for a concrete line in a poem after resolving the repeated scheme.
///
/// Positions here are poem-line-relative: for example, with repeated `ABAB`,
/// line `6` is a follower whose `leader_index` is `4`.
#[derive(Debug, PartialEq)]
pub enum LineRole {
    /// This line establishes the rhyme for its group in the current scheme cycle.
    Leader,
    /// This line has no rhyme constraint.
    Unconstrained,
    /// This line should rhyme with an earlier concrete line.
    Follower {
        /// The absolute line index of the leader this line should rhyme with.
        leader_index: usize,
    },
}

/// A line's rhyme role with any cross-line references resolved.
///
/// Unlike [`LineRole`], this type can borrow the concrete leader line needed to
/// check a follower. It is intended to be constructed temporarily by the caller
/// that owns the poem lines, then passed to [`RhymeScheme::check_line`].
pub enum ResolvedLineRole<'a> {
    /// This line establishes the rhyme for its group.
    Leader,
    /// This line has no rhyme constraint.
    Unconstrained,
    /// This line should rhyme with the borrowed leader line.
    Follower {
        /// The absolute line index of the leader.
        leader_index: usize,
        /// The concrete leader line to compare against.
        leader: &'a Line,
    },
}

#[derive(Debug)]
pub struct RhymeDistance {
    pub raw_distance: u32,
    pub syllable_count: usize,
    pub normalization: RhymeNormalization,
    pub normalization_denominator: f32,
    pub normalized_distance: f32,
    /// Step-by-step alignment of the winning comparison: side `a` is the
    /// leader rhyming part, side `b` the target slice it was compared against
    /// (which may span multiple words when the target's last word has fewer
    /// syllables than the leader's rhyming part). Both sequences are fully
    /// recoverable from here — see [`Self::leader_part`]/[`Self::target_part`].
    pub alignment: Vec<OwnedAlignmentStep>,
}

impl RhymeDistance {
    /// The leader rhyming part that produced the minimal distance, derived
    /// from the alignment's `a` sides.
    pub fn leader_part(&self) -> impl Iterator<Item = &Phoneme> {
        self.alignment.iter().filter_map(OwnedAlignmentStep::a)
    }

    /// The target phoneme slice the winning leader part was compared against,
    /// derived from the alignment's `b` sides.
    pub fn target_part(&self) -> impl Iterator<Item = &Phoneme> {
        self.alignment.iter().filter_map(OwnedAlignmentStep::b)
    }
}

/// Equality means "same measurement". `alignment` is provenance — which
/// comparison produced the measurement — and deliberately doesn't participate.
impl PartialEq for RhymeDistance {
    fn eq(&self, other: &Self) -> bool {
        self.raw_distance == other.raw_distance
            && self.syllable_count == other.syllable_count
            && self.normalization == other.normalization
            && self.normalization_denominator == other.normalization_denominator
            && self.normalized_distance == other.normalized_distance
    }
}

#[derive(Debug, PartialEq)]
pub enum RhymeCheckResult {
    Leader,
    Unconstrained,
    Follower {
        leader_index: usize,
        distance: RhymeDistance,
        passed: bool,
    },
}

impl RhymeScheme {
    pub fn new(s: &str, threshold: f32) -> Result<Self, ParseRhymeError> {
        Self::with_config(
            s,
            RhymeComparisonConfig {
                threshold,
                ..RhymeComparisonConfig::default()
            },
        )
    }

    pub fn with_config(
        s: &str,
        comparison_config: RhymeComparisonConfig,
    ) -> Result<Self, ParseRhymeError> {
        let mut map: HashMap<char, usize> = HashMap::new();
        let mut scheme = Vec::new();

        for (col, c) in s.chars().enumerate() {
            if c.is_whitespace() {
                continue;
            }

            let role = match c {
                '_' => SchemeRole::Unconstrained,
                c if c.is_ascii_alphabetic() => match map.get(&c) {
                    Some(&leader_position) => SchemeRole::Follower { leader_position },
                    None => {
                        map.insert(c, scheme.len());
                        SchemeRole::Leader
                    }
                },
                _ => return Err(ParseRhymeError::InvalidChar { c, col }),
            };

            scheme.push(role);
        }

        Ok(RhymeScheme {
            scheme,
            comparison_config,
        })
    }

    pub fn comparison_config(&self) -> RhymeComparisonConfig {
        self.comparison_config
    }

    /// Classifies a line's role in the rhyme scheme:
    /// - `Unconstrained`: the scheme doesn't require this line to rhyme with anything
    /// - `Leader`: this line is the first occurrence of its rhyme group in its cycle
    /// - `Follower`: this line should rhyme with the line at the given leader index
    pub(crate) fn line_role(&self, line_index: usize) -> LineRole {
        let pattern_len = self.scheme.len();
        let line_position = line_index % pattern_len;
        let cycle_start = line_index - line_position;

        match self.scheme[line_position] {
            SchemeRole::Leader => LineRole::Leader,
            SchemeRole::Unconstrained => LineRole::Unconstrained,
            SchemeRole::Follower { leader_position } => LineRole::Follower {
                leader_index: cycle_start + leader_position,
            },
        }
    }

    pub(crate) fn dependent_follower_indices(
        &self,
        line_index: usize,
        line_count: usize,
    ) -> Vec<usize> {
        let pattern_len = self.scheme.len();
        let line_position = line_index % pattern_len;
        let cycle_start = line_index - line_position;
        let cycle_end = (cycle_start + pattern_len).min(line_count);

        match self.line_role(line_index) {
            LineRole::Leader => (cycle_start..cycle_end)
                .filter(|&index| {
                    matches!(
                        self.line_role(index),
                        LineRole::Follower { leader_index } if leader_index == line_index
                    )
                })
                .collect(),
            LineRole::Unconstrained | LineRole::Follower { .. } => Vec::new(),
        }
    }

    /// Checks a line against an already resolved rhyme role.
    ///
    /// Follower roles compare (all of) the (possible) rhyming part(s) of the last word of the Leader against the same sized portion of Target, then check whether the minimum normalized distance is below
    /// this scheme's threshold.
    pub fn check_line<V, C>(
        &self,
        line: &Line,
        role: ResolvedLineRole<'_>,
        scorer: &RhymeDistanceScorer<V, C>,
    ) -> Result<RhymeCheckResult, RhymeCheckError>
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
        match role {
            ResolvedLineRole::Leader => Ok(RhymeCheckResult::Leader),
            ResolvedLineRole::Unconstrained => Ok(RhymeCheckResult::Unconstrained),
            ResolvedLineRole::Follower {
                leader_index,
                leader,
            } => {
                let distance = compare_rhyming_parts_with_normalization(
                    leader,
                    line,
                    scorer,
                    self.comparison_config.normalization,
                )?;
                let passed = distance.normalized_distance <= self.comparison_config.threshold;
                Ok(RhymeCheckResult::Follower {
                    leader_index,
                    distance,
                    passed,
                })
            }
        }
    }
}

fn rhyming_parts_of_last_word<'a>(
    line: &'a Line,
) -> Result<HashSet<RhymingPart<'a>>, RhymeCheckError> {
    let Some(word) = line.words.last() else {
        return Err(RhymeCheckError::EmptyLine);
    };
    let WordData::Known(pronunciations) = &word.data else {
        return Err(UnknownWordError {
            word: word.word.clone(),
        }
        .into());
    };
    Ok(pronunciations
        .iter()
        .map(|p| get_rhyming_part(&p.phonemes))
        .collect())
}

/// Returns all possible phoneme sequences representing the last `n` syllables of a sequence of words,
/// considering all pronunciation combinations across word boundaries.
///
/// Combinatoric possibilities without enough syllables are skipped.
///
/// If `n` is 0, returns an empty set.
///
/// Only returns an error if an unknown word is encountered.
fn last_n_syllables_of_line(
    words: &[WordEntry],
    n: usize,
) -> Result<HashSet<Vec<Phoneme>>, RhymeCheckError> {
    if n == 0 {
        return Ok(HashSet::from([vec![]]));
    }

    let Some(last_word) = words.last() else {
        return Ok(HashSet::new());
    };

    let WordData::Known(pronunciations) = &last_word.data else {
        return Err(UnknownWordError {
            word: last_word.word.clone(),
        }
        .into());
    };

    let preceding_words = &words[..words.len() - 1];
    let mut results = HashSet::new();

    for pronunciation in pronunciations {
        let syl_count = pronunciation.stress_pattern.len();

        if syl_count >= n {
            if let Some(slice) = get_last_n_syllables(&pronunciation.phonemes, n) {
                results.insert(slice.to_vec());
            } else {
                unreachable!();
            }
        } else {
            let needed = n - syl_count;

            let Ok(preceding_parts) = last_n_syllables_of_line(preceding_words, needed) else {
                continue;
            };

            for mut preceding in preceding_parts {
                preceding.extend_from_slice(&pronunciation.phonemes);
                results.insert(preceding);
            }
        }
    }

    Ok(results)
}

/// Compares (all of) the (possible) rhyming part(s) of the last word of Leader Line against the same sized portion of Target Line.
/// Returns the minimal normalized distance between the two sections.
fn compare_rhyming_parts_with_normalization<V, C>(
    leader: &Line,
    target: &Line,
    scorer: &RhymeDistanceScorer<V, C>,
    normalization: RhymeNormalization,
) -> Result<RhymeDistance, RhymeCheckError>
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    // The winning candidate: measurement plus the two slices it came from.
    // The slices only exist to run the traceback below; the returned
    // RhymeDistance carries them inside `alignment` instead.
    struct Candidate {
        raw_distance: u32,
        syllable_count: usize,
        normalization_denominator: f32,
        normalized_distance: f32,
        leader_part: Vec<Phoneme>,
        target_part: Vec<Phoneme>,
    }

    let leader_parts = rhyming_parts_of_last_word(leader)?;

    let mut min: Option<Candidate> = None;

    for rp_leader in leader_parts
        .iter()
        .filter(|rp_leader| rp_leader.syllable_count > 0)
    {
        let syllable_count = rp_leader.syllable_count;

        let target_parts = last_n_syllables_of_line(&target.words, syllable_count)?;

        for target_phonemes in target_parts {
            let raw_distance = scorer.distance(rp_leader.phonemes, &target_phonemes);
            let normalization_denominator = normalization.denominator(
                syllable_count,
                rp_leader.phonemes.len(),
                target_phonemes.len(),
            );
            let normalized_distance = raw_distance as f32 / normalization_denominator;

            let is_new_min = min
                .as_ref()
                .is_none_or(|current| normalized_distance < current.normalized_distance);
            if is_new_min {
                min = Some(Candidate {
                    raw_distance,
                    syllable_count,
                    normalization_denominator,
                    normalized_distance,
                    leader_part: rp_leader.phonemes.to_vec(),
                    target_part: target_phonemes,
                });
            }
        }
    }

    let winner = min.ok_or(RhymeCheckError::NotEnoughSyllablesInTarget)?;

    // Candidates are scored with the cheap `distance()` (no edit recording);
    // the traceback runs once, for the winning pair only.
    let alignment = scorer
        .align(&winner.leader_part, &winner.target_part)
        .alignment()
        .iter()
        .map(|step| OwnedAlignmentStep::from(*step))
        .collect();

    Ok(RhymeDistance {
        raw_distance: winner.raw_distance,
        syllable_count: winner.syllable_count,
        normalization,
        normalization_denominator: winner.normalization_denominator,
        normalized_distance: winner.normalized_distance,
        alignment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmuDictionary;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();
    static SCORER: OnceLock<RhymeDistanceScorer> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    fn scorer() -> &'static RhymeDistanceScorer {
        SCORER.get_or_init(RhymeDistanceScorer::new)
    }

    fn compare_rhyming_parts<V, C>(
        leader: &Line,
        target: &Line,
        scorer: &RhymeDistanceScorer<V, C>,
    ) -> Result<RhymeDistance, RhymeCheckError>
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
        compare_rhyming_parts_with_normalization(
            leader,
            target,
            scorer,
            RhymeNormalization::PerSyllable,
        )
    }

    fn ph(arpa: &[&str]) -> Vec<Phoneme> {
        arpa.iter().map(|s| s.parse().unwrap()).collect()
    }

    fn pronunciation(arpa: &[&str]) -> crate::line::Pronunciation {
        let phonemes = ph(arpa);
        let stress_pattern = phonemes
            .iter()
            .filter_map(|p| match p {
                Phoneme::Vowel(v) => Some(v.stress),
                Phoneme::Consonant(_) => None,
            })
            .collect();

        crate::line::Pronunciation {
            phonemes: phonemes.into_boxed_slice(),
            stress_pattern,
        }
    }

    fn known_word(word: &str, pronunciations: &[&[&str]]) -> WordEntry {
        WordEntry {
            word: word.to_string(),
            data: WordData::Known(
                pronunciations
                    .iter()
                    .map(|arpa| pronunciation(arpa))
                    .collect(),
            ),
        }
    }

    fn zero_distance(syllable_count: usize) -> RhymeDistance {
        RhymeDistance {
            raw_distance: 0,
            syllable_count,
            normalization: RhymeNormalization::PerSyllable,
            normalization_denominator: syllable_count as f32,
            normalized_distance: 0.0,
            // Equality ignores provenance, so expected values leave it empty.
            alignment: Vec::new(),
        }
    }

    fn default_zero_distance(syllable_count: usize, phoneme_count: usize) -> RhymeDistance {
        RhymeDistance {
            normalization: RhymeNormalization::default(),
            normalization_denominator: phoneme_count as f32,
            ..zero_distance(syllable_count)
        }
    }

    // --- rhyming_parts_of_last_word ---

    #[test]
    fn rhyming_parts_empty_line_returns_empty() {
        let line = Line::new("", dict());
        assert_eq!(
            rhyming_parts_of_last_word(&line),
            Err(RhymeCheckError::EmptyLine)
        );
    }

    #[test]
    fn rhyming_parts_unknown_word_returns_empty() {
        let line = Line::new("xyzzy", dict());
        assert_eq!(
            rhyming_parts_of_last_word(&line),
            Err(RhymeCheckError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string(),
            }))
        );
    }

    #[test]
    fn rhyming_parts_single_known_word() {
        // "cat" = K AE1 T, rhyming part starts at last stressed vowel AE1
        let line = Line::new("cat", dict());
        let parts = rhyming_parts_of_last_word(&line).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts.iter().next().unwrap().phonemes, ph(&["AE1", "T"]));
    }

    #[test]
    fn rhyming_parts_uses_last_word() {
        // "world" = W ER1 L D, rhyming part starts at ER1
        let line = Line::new("hello world", dict());
        let parts = rhyming_parts_of_last_word(&line).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts.iter().next().unwrap().phonemes,
            ph(&["ER1", "L", "D"])
        );
    }

    #[test]
    fn rhyming_parts_multiple_distinct_parts() {
        // "contract" has two pronunciations with primary stress on different syllables:
        // K AA1 N T R AE2 K T → last primary stressed = AA1 → [AA1, N, T, R, AE2, K, T]
        // K AH0 N T R AE1 K T → last primary stressed = AE1 → [AE1, K, T]
        let line = Line::new("contract", dict());
        let parts = rhyming_parts_of_last_word(&line).unwrap();
        assert_eq!(parts.len(), 2);
        let phoneme_sets: HashSet<&[Phoneme]> = parts.iter().map(|rp| rp.phonemes).collect();
        assert!(phoneme_sets.contains(ph(&["AA1", "N", "T", "R", "AE2", "K", "T"]).as_slice()));
        assert!(phoneme_sets.contains(ph(&["AE1", "K", "T"]).as_slice()));
    }

    #[test]
    fn rhyming_parts_multiple_pronunciations_deduplicated() {
        // "hello" has two pronunciations: HH AH0 L OW1 and HH EH0 L OW1
        // both share the same rhyming part [OW1], so the HashSet deduplicates to 1 entry
        let line = Line::new("hello", dict());
        let parts = rhyming_parts_of_last_word(&line).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts.iter().next().unwrap().phonemes, ph(&["OW1"]));
    }

    // --- last_n_syllables_of_line ---

    #[test]
    fn last_n_syllables_zero_returns_empty_vec() {
        let line = Line::new("hello world", dict());
        let result = last_n_syllables_of_line(&line.words, 0);
        assert_eq!(result, Ok(HashSet::from([vec![]])));
    }

    #[test]
    fn last_n_syllables_within_last_word() {
        // "world" = W ER1 L D — last 1 syllable starts at ER1
        let line = Line::new("hello world", dict());
        let result = last_n_syllables_of_line(&line.words, 1);
        assert_eq!(result, Ok(HashSet::from([ph(&["ER1", "L", "D"])])));
    }

    #[test]
    fn last_n_syllables_crosses_word_boundary() {
        // "world" has 1 syllable; need 1 more from "hello" (both pronunciations end in OW1)
        // result: [OW1] prepended to [W, ER1, L, D]
        let line = Line::new("hello world", dict());
        let result = last_n_syllables_of_line(&line.words, 2);
        assert_eq!(
            result,
            Ok(HashSet::from([ph(&["OW1", "W", "ER1", "L", "D"])]))
        );
    }

    #[test]
    fn last_n_syllables_multiple_pronunciations_across_boundary() {
        // "contract fire": both words have multiple pronunciations, producing 4 combinations.
        // FIRE has two pronunciations:
        //   F AY1 ER0 (2 syllables) → needs 1 more from CONTRACT
        //   F AY1 R   (1 syllable)  → needs 2 more from CONTRACT
        // CONTRACT has two pronunciations:
        //   K AA1 N T R AE2 K T → last 1 syl = [AE2, K, T], last 2 syl = [AA1, N, T, R, AE2, K, T]
        //   K AH0 N T R AE1 K T → last 1 syl = [AE1, K, T], last 2 syl = [AH0, N, T, R, AE1, K, T]
        let line = Line::new("contract fire", dict());
        let result = last_n_syllables_of_line(&line.words, 3).unwrap();
        assert_eq!(result.len(), 4);
        assert!(result.contains(&ph(&["AE2", "K", "T", "F", "AY1", "ER0"])));
        assert!(result.contains(&ph(&["AE1", "K", "T", "F", "AY1", "ER0"])));
        assert!(result.contains(&ph(&[
            "AA1", "N", "T", "R", "AE2", "K", "T", "F", "AY1", "R"
        ])));
        assert!(result.contains(&ph(&[
            "AH0", "N", "T", "R", "AE1", "K", "T", "F", "AY1", "R"
        ])));
    }

    #[test]
    fn last_n_syllables_unknown_word_returns_empty() {
        let line = Line::new("xyzzy", dict());
        let result = last_n_syllables_of_line(&line.words, 1);
        assert_eq!(
            result,
            Err(RhymeCheckError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string(),
            }))
        );
    }

    #[test]
    fn last_n_syllables_more_than_available_returns_empty() {
        // "cat" has 1 syllable, asking for 2 with no preceding words → empty
        let line = Line::new("cat", dict());
        let result = last_n_syllables_of_line(&line.words, 2);
        assert_eq!(result, Ok(HashSet::new()));
    }

    #[test]
    fn last_n_syllables_unknown_word_in_chain_returns_empty() {
        // "world" has 1 syllable, asking for 2 requires "xyzzy" which is unknown
        let line = Line::new("xyzzy world", dict());
        let result = last_n_syllables_of_line(&line.words, 2);
        assert_eq!(result, Ok(HashSet::new()));
    }

    // --- compare_rhyming_parts ---

    #[test]
    fn normalization_denominators_match_their_units() {
        assert_eq!(RhymeNormalization::None.denominator(2, 4, 5), 1.0);
        assert_eq!(RhymeNormalization::PerSyllable.denominator(2, 4, 5), 2.0);
        assert_eq!(RhymeNormalization::PerMeanPhoneme.denominator(2, 4, 5), 4.5);
    }

    #[test]
    fn compare_rhyming_parts_identical_words_score_zero() {
        let a = Line::new("cat", dict());
        let b = Line::new("cat", dict());
        assert_eq!(
            compare_rhyming_parts(&a, &b, scorer()),
            Ok(zero_distance(1))
        );
    }

    #[test]
    fn compare_rhyming_parts_records_compared_parts() {
        // Equality on RhymeDistance ignores provenance, so assert on the
        // accessors directly: cat = K AE1 T, hat = HH AE1 T — the winning
        // comparison is leader part [AE1 T] against target slice [AE1 T],
        // both recoverable from the alignment's sides.
        let cat = Line::new("cat", dict());
        let hat = Line::new("hat", dict());
        let distance = compare_rhyming_parts(&cat, &hat, scorer()).unwrap();
        assert_eq!(
            distance.leader_part().cloned().collect::<Vec<_>>(),
            ph(&["AE1", "T"])
        );
        assert_eq!(
            distance.target_part().cloned().collect::<Vec<_>>(),
            ph(&["AE1", "T"])
        );
    }

    #[test]
    fn compare_rhyming_parts_records_alignment_of_winning_pair() {
        // cat = K AE1 T, cot = K AA1 T: rhyming parts AE1 T vs AA1 T align as
        // a vowel substitution followed by a consonant match.
        let cat = Line::new("cat", dict());
        let cot = Line::new("cot", dict());
        let distance = compare_rhyming_parts(&cat, &cot, scorer()).unwrap();
        assert_eq!(
            distance.alignment,
            vec![
                OwnedAlignmentStep::Substitution {
                    a: ph(&["AE1"])[0].clone(),
                    b: ph(&["AA1"])[0].clone(),
                },
                OwnedAlignmentStep::Match {
                    a: ph(&["T"])[0].clone(),
                    b: ph(&["T"])[0].clone(),
                },
            ]
        );
    }

    #[test]
    fn compare_rhyming_parts_rhyming_words_low_score() {
        // "cat" and "hat" rhyme — normalized score should be lower than a non-rhyme
        let cat = Line::new("cat", dict());
        let hat = Line::new("hat", dict());
        let non_rhyme = Line::new("hello", dict());
        let rhyme_score = compare_rhyming_parts(&cat, &hat, scorer()).unwrap();
        let non_rhyme_score = compare_rhyming_parts(&cat, &non_rhyme, scorer()).unwrap();
        assert!(rhyme_score.normalized_distance < non_rhyme_score.normalized_distance);
    }

    #[test]
    fn compare_rhyming_parts_unknown_word_returns_none() {
        let a = Line::new("xyzzy", dict());
        let b = Line::new("cat", dict());
        assert_eq!(
            compare_rhyming_parts(&a, &b, scorer()),
            Err(RhymeCheckError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string(),
            }))
        );
        assert_eq!(
            compare_rhyming_parts(&b, &a, scorer()),
            Err(RhymeCheckError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string(),
            }))
        );
    }

    #[test]
    fn compare_rhyming_parts_empty_line_returns_none() {
        let a = Line::new("", dict());
        let b = Line::new("cat", dict());
        assert_eq!(
            compare_rhyming_parts(&a, &b, scorer()),
            Err(RhymeCheckError::EmptyLine)
        );
    }

    #[test]
    fn compare_rhyming_parts_finds_longer_match_across_follower_pronunciation_combinations() {
        let leader = Line {
            words: vec![known_word(
                "leader",
                &[&["AE1", "T"][..], &["IY1", "Z", "AH0"][..]],
            )],
        };
        let follower = Line {
            words: vec![
                known_word("follower_prefix", &[&["AA1", "R"][..], &["IY1", "Z"][..]]),
                known_word("follower_last", &[&["EH0"][..], &["AH0"][..]]),
            ],
        };

        assert_eq!(
            compare_rhyming_parts(&leader, &follower, scorer()),
            Ok(zero_distance(2))
        );
    }

    #[test]
    fn compare_rhyming_parts_finds_shorter_match_across_follower_pronunciation_combinations() {
        let leader = Line {
            words: vec![known_word(
                "leader",
                &[
                    &["AE1", "T", "AH0"][..],
                    &["IY1", "Z", "AH0", "N", "OW0"][..],
                ],
            )],
        };
        let follower = Line {
            words: vec![
                known_word(
                    "follower_prefix",
                    &[&["OW1", "K", "ER0"][..], &["UH0", "M", "AE1", "T"][..]],
                ),
                known_word("follower_last", &[&["IH0"][..], &["AH0"][..]]),
            ],
        };

        assert_eq!(
            compare_rhyming_parts(&leader, &follower, scorer()),
            Ok(zero_distance(2))
        );
    }

    // --- RhymeScheme::new ---

    #[test]
    fn rhyme_scheme_basic_pattern() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 },
                SchemeRole::Follower { leader_position: 1 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_ignores_whitespace() {
        let rs = RhymeScheme::new("A B\nA B", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 },
                SchemeRole::Follower { leader_position: 1 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_underscore_is_unconstrained() {
        let rs = RhymeScheme::new("A_BA", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Unconstrained,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_repeated_letters_follow_first_appearance() {
        let rs = RhymeScheme::new("ZAZA", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 },
                SchemeRole::Follower { leader_position: 1 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_threshold_stored() {
        let rs = RhymeScheme::new("AABB", 7.5).unwrap();
        assert_eq!(rs.comparison_config().threshold, 7.5);
        assert_eq!(
            rs.comparison_config().normalization,
            RhymeNormalization::PerMeanPhoneme
        );
    }

    #[test]
    fn rhyme_scheme_stores_explicit_comparison_config() {
        let config = RhymeComparisonConfig {
            threshold: 3.0,
            normalization: RhymeNormalization::PerMeanPhoneme,
        };
        let rs = RhymeScheme::with_config("AA", config).unwrap();
        assert_eq!(rs.comparison_config(), config);
    }

    #[test]
    fn rhyme_scheme_invalid_char_returns_error_with_column() {
        let err = match RhymeScheme::new("A B1", 8.0) {
            Ok(_) => panic!("expected invalid rhyme character"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            ParseRhymeError::InvalidChar { c: '1', col: 3 }
        ));
        assert_eq!(err.to_string(), "invalid rhyme character '1' at column 4");
    }

    // --- line_role ---

    #[test]
    fn line_role_first_occurrence_is_leader() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        assert_eq!(rs.line_role(0), LineRole::Leader);
        assert_eq!(rs.line_role(1), LineRole::Leader);
    }

    #[test]
    fn line_role_follower_points_to_leader() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        assert_eq!(rs.line_role(2), LineRole::Follower { leader_index: 0 });
        assert_eq!(rs.line_role(3), LineRole::Follower { leader_index: 1 });
    }

    #[test]
    fn line_role_resets_per_cycle() {
        // Each repetition of the pattern is its own group of leaders/followers.
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        // line 4 starts cycle 2; it's the new leader for its A group
        assert_eq!(rs.line_role(4), LineRole::Leader);
        assert_eq!(rs.line_role(5), LineRole::Leader);
        // line 6 is the second A of cycle 2 → leader is line 4
        assert_eq!(rs.line_role(6), LineRole::Follower { leader_index: 4 });
        assert_eq!(rs.line_role(7), LineRole::Follower { leader_index: 5 });
    }

    #[test]
    fn line_role_unconstrained_returns_unconstrained() {
        let rs = RhymeScheme::new("A_BA", 8.0).unwrap();
        assert_eq!(rs.line_role(1), LineRole::Unconstrained);
    }

    // --- dependent_follower_indices ---

    #[test]
    fn dependent_follower_indices_returns_followers_in_same_cycle() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();

        assert_eq!(rs.dependent_follower_indices(0, 4), vec![2]);
        assert_eq!(rs.dependent_follower_indices(1, 4), vec![3]);
    }

    #[test]
    fn dependent_follower_indices_resets_per_cycle() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();

        assert_eq!(rs.dependent_follower_indices(4, 8), vec![6]);
        assert_eq!(rs.dependent_follower_indices(5, 8), vec![7]);
    }

    #[test]
    fn dependent_follower_indices_returns_multiple_followers() {
        let rs = RhymeScheme::new("AAAA", 8.0).unwrap();

        assert_eq!(rs.dependent_follower_indices(0, 4), vec![1, 2, 3]);
    }

    #[test]
    fn dependent_follower_indices_stops_at_line_count() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();

        assert_eq!(rs.dependent_follower_indices(0, 3), vec![2]);
        assert_eq!(rs.dependent_follower_indices(1, 3), Vec::<usize>::new());
    }

    #[test]
    fn dependent_follower_indices_returns_empty_for_non_leaders() {
        let rs = RhymeScheme::new("A_BA", 8.0).unwrap();

        assert_eq!(rs.dependent_follower_indices(1, 4), Vec::<usize>::new());
        assert_eq!(rs.dependent_follower_indices(3, 4), Vec::<usize>::new());
    }

    // --- RhymeScheme::check_line ---

    #[test]
    fn check_line_leader_returns_leader() {
        let rs = RhymeScheme::new("ABAB", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("dog", dict())];

        assert_eq!(
            rs.check_line(&lines[0], ResolvedLineRole::Leader, scorer()),
            Ok(RhymeCheckResult::Leader)
        );
        assert_eq!(
            rs.check_line(&lines[1], ResolvedLineRole::Leader, scorer()),
            Ok(RhymeCheckResult::Leader)
        );
    }

    #[test]
    fn check_line_unconstrained_returns_unconstrained() {
        let rs = RhymeScheme::new("A_", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("orange", dict())];

        assert_eq!(
            rs.check_line(&lines[1], ResolvedLineRole::Unconstrained, scorer()),
            Ok(RhymeCheckResult::Unconstrained)
        );
    }

    #[test]
    fn check_line_follower_uses_resolved_leader() {
        let rs = RhymeScheme::new("ABAB", 1.0).unwrap();
        let lines = vec![
            Line::new("cat", dict()),
            Line::new("dog", dict()),
            Line::new("hat", dict()),
            Line::new("fog", dict()),
        ];

        assert_eq!(
            rs.check_line(
                &lines[2],
                ResolvedLineRole::Follower {
                    leader_index: 0,
                    leader: &lines[0],
                },
                scorer()
            ),
            Ok(RhymeCheckResult::Follower {
                leader_index: 0,
                distance: default_zero_distance(1, 2),
                passed: true,
            })
        );
        assert_eq!(
            rs.check_line(
                &lines[3],
                ResolvedLineRole::Follower {
                    leader_index: 1,
                    leader: &lines[1],
                },
                scorer()
            ),
            Ok(RhymeCheckResult::Follower {
                leader_index: 1,
                distance: default_zero_distance(1, 2),
                passed: true,
            })
        );
    }

    #[test]
    fn check_line_follower_uses_resolved_cycle_leader() {
        let rs = RhymeScheme::new("ABAB", 1.0).unwrap();
        let lines = vec![
            Line::new("cat", dict()),
            Line::new("dog", dict()),
            Line::new("hat", dict()),
            Line::new("fog", dict()),
            Line::new("moon", dict()),
            Line::new("light", dict()),
            Line::new("tune", dict()),
            Line::new("night", dict()),
        ];

        assert_eq!(
            rs.check_line(
                &lines[6],
                ResolvedLineRole::Follower {
                    leader_index: 4,
                    leader: &lines[4],
                },
                scorer()
            ),
            Ok(RhymeCheckResult::Follower {
                leader_index: 4,
                distance: default_zero_distance(1, 2),
                passed: true,
            })
        );
        assert_eq!(
            rs.check_line(
                &lines[7],
                ResolvedLineRole::Follower {
                    leader_index: 5,
                    leader: &lines[5],
                },
                scorer()
            ),
            Ok(RhymeCheckResult::Follower {
                leader_index: 5,
                distance: default_zero_distance(1, 2),
                passed: true,
            })
        );
    }

    #[test]
    fn check_line_follower_unknown_word_returns_distance_error() {
        let rs = RhymeScheme::new("AA", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("xyzzy", dict())];

        assert_eq!(
            rs.check_line(
                &lines[1],
                ResolvedLineRole::Follower {
                    leader_index: 0,
                    leader: &lines[0],
                },
                scorer()
            ),
            Err(RhymeCheckError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string(),
            }))
        );
    }

    #[test]
    fn check_line_follower_empty_line_returns_distance_error() {
        let rs = RhymeScheme::new("AA", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("", dict())];

        assert_eq!(
            rs.check_line(
                &lines[1],
                ResolvedLineRole::Follower {
                    leader_index: 0,
                    leader: &lines[0],
                },
                scorer()
            ),
            Err(RhymeCheckError::NotEnoughSyllablesInTarget)
        );
    }
}
