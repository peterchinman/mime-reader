//! The library's long-lived facade: a pronouncing dictionary bundled with a
//! rhyme-distance scorer, plus the dictionary-wide operations (like
//! [`Analyzer::find_rhymes_intersection_by_words`]) that need both. Poem
//! analysis lives in [`crate::poem`]; an [`Analyzer`] supplies the shared
//! state poems borrow.

use std::{collections::HashSet, ops::RangeInclusive};

use crate::{
    CmuDictionary, Phoneme, PronouncingDictionary, RhymeDistanceConfig, RhymeDistanceScorer,
    RhymePairScore, RhymeScoreNormalization, UnknownWordError,
    consonant_distance::{
        AnalyticConsonantDistance, AnalyticConsonantDistanceConfig, ConsonantDistanceScorer,
    },
    phoneme::{get_rhyming_part, get_syllable_count},
    rhyme_distance::score_rhyme_pair,
    vowel_distance::{VowelDistanceScorer, VowelHexGraph, VowelHexGraphConfig},
};

pub struct Analyzer<V = VowelHexGraph, C = AnalyticConsonantDistance, D = CmuDictionary>
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
    D: PronouncingDictionary,
{
    rhyme_distance_scorer: RhymeDistanceScorer<V, C>,
    dict: D,
}

/// Which portion of each pronunciation is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    WholeWord,
    RhymingPart,
}

/// The best scoring configuration for one subject pronunciation across all
/// targets.
///
/// The subject pronunciation and target comparison slices borrow from their
/// respective inputs, which may have different owners.
#[derive(Debug)]
pub struct ConsistentScores<'s, 't> {
    pub best_subject_pronunciation: &'s [Phoneme],
    /// `best_scored_targets[i]` is the winning comparison slice for
    /// `targets[i]`, together with its score.
    pub best_scored_targets: Vec<(&'t [Phoneme], RhymePairScore)>,
}

impl<'s, 't> ConsistentScores<'s, 't> {
    pub fn to_owned(&self) -> ConsistentScoresOwned {
        ConsistentScoresOwned {
            best_subject_pronunciation: self.best_subject_pronunciation.into(),
            best_scored_targets: self
                .best_scored_targets
                .iter()
                .map(|&(pronunciation, score)| (Box::from(pronunciation), score))
                .collect(),
        }
    }
}

#[derive(Debug)]
pub struct ConsistentScoresOwned {
    pub best_subject_pronunciation: Box<[Phoneme]>,
    pub best_scored_targets: Vec<(Box<[Phoneme]>, RhymePairScore)>,
}

/// One target of an [`Analyzer::find_rhymes_intersection_by_words`] scan: a word plus the rhyme distance range around it.
#[derive(Debug, Clone)]
pub struct WordTarget<'a> {
    pub word: &'a str,
    pub distance_range: RangeInclusive<f32>,
}

/// A dictionary word inside every target's distance range — a lean,
/// borrowed scan summary.
#[derive(Debug)]
pub struct RhymeIntersectionMatch<'d, 't> {
    pub word: &'d str,
    /// The shared subject pronunciation every score uses.
    pub scores: ConsistentScores<'d, 't>,
}

/// An owned word-scan result, including any derived target pronunciations.
#[derive(Debug)]
pub struct RhymeIntersectionMatchOwned {
    pub word: String,
    pub scores: ConsistentScoresOwned,
}

impl<V, C, D> Analyzer<V, C, D>
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
    D: PronouncingDictionary,
{
    pub fn new(dict: D, rhyme_distance_scorer: RhymeDistanceScorer<V, C>) -> Self {
        Self {
            dict,
            rhyme_distance_scorer,
        }
    }

    pub fn dict(&self) -> &D {
        &self.dict
    }

    pub fn rhyme_distance_scorer(&self) -> &RhymeDistanceScorer<V, C> {
        &self.rhyme_distance_scorer
    }

    pub fn set_rhyme_distance_config(&mut self, config: RhymeDistanceConfig) {
        self.rhyme_distance_scorer.set_config(config);
    }

    /// Scores a `subject` word against every `target` word using a *single*
    /// pronunciation of `subject` — the one minimizing the summed
    /// normalized distance — so the per-target scores are mutually
    /// consistent. Repeated single-target calls may each pick a different
    /// pronunciation, which lets a subject with pronunciation variants sit
    /// "close" to several targets no single pronunciation is close to.
    /// `compare` is applied to both the subject and target pronunciations.
    /// Word pronunciations follow the dictionary's pronunciation mode.
    ///
    /// Returns `None` when no pronunciation of `subject` is comparable to every
    /// target. An empty `targets` slice returns `None` without looking up
    /// `subject`.
    pub fn calculate_consistent_score_between_words(
        &self,
        subject: &str,
        targets: &[&str],
        normalization: RhymeScoreNormalization,
        compare: Compare,
    ) -> Result<Option<ConsistentScoresOwned>, UnknownWordError> {
        if targets.is_empty() {
            return Ok(None);
        }

        let subject_pronunciations = self
            .dict
            .pronunciations_for(subject)
            .map(|word| word.pronunciations)
            .ok_or_else(|| UnknownWordError {
                word: subject.to_string(),
            })?;
        let target_pronunciations = targets
            .iter()
            .map(|target| {
                self.dict
                    .pronunciations_for(target)
                    .map(|word| word.pronunciations)
                    .ok_or_else(|| UnknownWordError {
                        word: target.to_string(),
                    })
            })
            .collect::<Result<Vec<_>, UnknownWordError>>()?;
        let pronunciation_targets: Vec<PronunciationsTarget<'_>> = target_pronunciations
            .iter()
            .map(|pronunciations| {
                PronunciationsTarget::new(
                    pronunciations.iter().map(|p| p.as_ref()),
                    0.0..=f32::INFINITY,
                )
            })
            .collect();
        let comparison_targets = prepare_comparison_targets(&pronunciation_targets, compare);

        Ok(self
            .best_consistent_scores(
                subject_pronunciations.as_ref(),
                &comparison_targets,
                normalization,
                compare,
            )
            .map(|(_, consistent)| consistent.to_owned()))
    }

    /// Scans the dictionary for words inside *every* target's distance range. If a single target word is provided this finds rhymes of that particular word within the target distance range.
    ///
    /// Matches are sorted by the sum of their normalized distances,
    /// nearest first (ties broken alphabetically). All target words are excluded from the
    /// results. An empty `targets` slice returns no matches (rather than
    /// the vacuous whole dictionary).
    /// Targets follow the dictionary's pronunciation mode; candidates are always
    /// dictionary entries. Results own their scores so inferred pronunciations
    /// remain valid after the scan.
    ///
    /// TODO: this is a naive implementation that simply iterates the entire dict. We can imagine faster implementations that would bucket the dictionary by vowel order.
    pub fn find_rhymes_intersection_by_words(
        &self,
        targets: &[WordTarget],
        normalization: RhymeScoreNormalization,
        compare: Compare,
    ) -> Result<Vec<RhymeIntersectionMatchOwned>, UnknownWordError> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let excluded_words: Vec<&str> = targets.iter().map(|t| t.word).collect();
        let target_pronunciations = targets
            .iter()
            .map(|target| {
                self.dict
                    .pronunciations_for(target.word)
                    .map(|word| word.pronunciations)
                    .ok_or_else(|| UnknownWordError {
                        word: target.word.to_string(),
                    })
            })
            .collect::<Result<Vec<_>, UnknownWordError>>()?;
        let pronunciation_targets = targets
            .iter()
            .zip(&target_pronunciations)
            .map(|(target, pronunciations)| {
                PronunciationsTarget::new(
                    pronunciations
                        .iter()
                        .map(|pronunciation| pronunciation.as_ref()),
                    target.distance_range.clone(),
                )
            })
            .collect::<Vec<_>>();

        Ok(self
            .find_rhymes_intersection_by_pronunciations(
                &pronunciation_targets,
                Some(&excluded_words),
                normalization,
                compare,
            )
            .into_iter()
            .map(|m| RhymeIntersectionMatchOwned {
                word: m.word.to_string(),
                scores: m.scores.to_owned(),
            })
            .collect())
    }

    /// An empty `targets` slice returns no matches. When every target uses
    /// `0.0..=f32::INFINITY`, returns every non-excluded dictionary entry with
    /// at least one pronunciation, ranked by the sum of its normalized rhyme
    /// distances to the targets. `compare` selects the part of both the
    /// dictionary and target pronunciations that is scored.
    pub fn find_rhymes_intersection_by_pronunciations<'d, 't>(
        &'d self,
        targets: &[PronunciationsTarget<'t>],
        excluded_words: Option<&[&str]>,
        normalization: RhymeScoreNormalization,
        compare: Compare,
    ) -> Vec<RhymeIntersectionMatch<'d, 't>> {
        if targets.is_empty() {
            return Vec::new();
        }

        let mut ranked_matches: Vec<(f32, RhymeIntersectionMatch<'d, 't>)> = Vec::new();
        let comparison_targets = prepare_comparison_targets(targets, compare);

        for (dict_word, dict_pronunciations) in self.dict.entries() {
            if excluded_words.is_some_and(|excluded| {
                excluded
                    .iter()
                    .any(|word| dict_word.eq_ignore_ascii_case(word))
            }) {
                continue;
            }
            if let Some((total, consistent)) = self.best_consistent_scores(
                dict_pronunciations,
                &comparison_targets,
                normalization,
                compare,
            ) {
                ranked_matches.push((
                    total,
                    RhymeIntersectionMatch {
                        word: dict_word,
                        scores: consistent,
                    },
                ));
            }
        }

        // Alphabetical tie-break keeps equal-distance words in a stable order
        // (the pre-sort order is HashMap iteration, which is arbitrary).
        ranked_matches.sort_by(|(a_total, a), (b_total, b)| {
            a_total.total_cmp(b_total).then_with(|| a.word.cmp(b.word))
        });

        ranked_matches
            .into_iter()
            .map(|(_, rhyme_match)| rhyme_match)
            .collect()
    }

    /// Each subject pronunciation is scored per target by its best in-range
    /// pairing with that target's comparison parts (skipping pronunciations
    /// that can't reach every target within its range), and the pronunciation
    /// minimizing the summed normalized distance wins. Each per-target entry
    /// carries the winning comparison slice.
    ///
    /// TODO: does "minimizing the summed distance" actually make the most sense here? I could maybe imagine scenarios where you want to minimize, for example, "average distance"?
    fn best_consistent_scores<'s, 't>(
        &self,
        subject_pronunciations: &'s [Box<[Phoneme]>],
        targets: &[ComparisonTarget<'t>],
        normalization: RhymeScoreNormalization,
        compare: Compare,
    ) -> Option<(f32, ConsistentScores<'s, 't>)> {
        let mut best: Option<(f32, ConsistentScores<'s, 't>)> = None;
        // Reused across pronunciations
        let mut scores: Vec<(&'t [Phoneme], RhymePairScore)> = Vec::new();

        'pronunciations: for subject_pronunciation in subject_pronunciations {
            let subject_comparison = comparison_part(subject_pronunciation, compare);
            scores.clear();
            for target in targets {
                let mut target_best: Option<(&'t [Phoneme], RhymePairScore)> = None;
                for &target_comparison in target.parts.iter() {
                    let score = score_rhyme_pair(
                        &self.rhyme_distance_scorer,
                        subject_comparison,
                        target_comparison,
                    );
                    if !target
                        .distance_range
                        .contains(&score.normalized(normalization))
                    {
                        continue;
                    }
                    let is_new_best = target_best.as_ref().is_none_or(|(_, b)| {
                        score.normalized(normalization) < b.normalized(normalization)
                    });
                    if is_new_best {
                        target_best = Some((target_comparison, score));
                    }
                }
                match target_best {
                    Some(scored) => scores.push(scored),
                    // This pronunciation can't reach every target in range.
                    None => continue 'pronunciations,
                }
            }
            let total: f32 = scores
                .iter()
                .map(|(_, score)| score.normalized(normalization))
                .sum();
            if best.as_ref().is_none_or(|(t, _)| total < *t) {
                best = Some((
                    total,
                    ConsistentScores {
                        best_subject_pronunciation: subject_pronunciation,
                        best_scored_targets: std::mem::take(&mut scores),
                    },
                ));
            }
        }
        best
    }
}

/// One target: its full pronunciations and the admissible distance range.
///
/// The requested [`Compare`] mode is applied later, when the target is scored.
pub struct PronunciationsTarget<'p> {
    pronunciations: Box<[&'p [Phoneme]]>,
    distance_range: RangeInclusive<f32>,
}

impl<'p> PronunciationsTarget<'p> {
    /// Creates a target from complete pronunciations.
    ///
    /// # Panics
    ///
    /// Panics if no pronunciations are supplied or if any supplied
    /// pronunciation has no syllables. These invariants ensure every target
    /// has a comparison part and normalized scores have a nonzero denominator.
    pub fn new(
        pronunciations: impl IntoIterator<Item = &'p [Phoneme]>,
        distance_range: RangeInclusive<f32>,
    ) -> Self {
        let pronunciations: Box<[&'p [Phoneme]]> = pronunciations.into_iter().collect();
        assert!(
            !pronunciations.is_empty(),
            "a pronunciation target must contain at least one pronunciation"
        );
        assert!(
            pronunciations
                .iter()
                .all(|pronunciation| get_syllable_count(pronunciation) > 0),
            "every target pronunciation must contain at least one syllable"
        );

        Self {
            pronunciations,
            distance_range,
        }
    }
}

/// A target projected into the comparison slices used throughout one scan.
struct ComparisonTarget<'p> {
    parts: Box<[&'p [Phoneme]]>,
    distance_range: RangeInclusive<f32>,
}

fn prepare_comparison_targets<'p>(
    targets: &[PronunciationsTarget<'p>],
    compare: Compare,
) -> Vec<ComparisonTarget<'p>> {
    targets
        .iter()
        .map(|target| ComparisonTarget {
            parts: get_comparison_parts(target.pronunciations.iter().copied(), compare),
            distance_range: target.distance_range.clone(),
        })
        .collect()
}

fn comparison_part(pronunciation: &[Phoneme], compare: Compare) -> &[Phoneme] {
    match compare {
        Compare::RhymingPart => get_rhyming_part(pronunciation).phonemes,
        Compare::WholeWord => pronunciation,
    }
}

/// The distinct comparison parts across a word's pronunciations, deduplicated.
fn get_comparison_parts<'a>(
    pronunciations: impl IntoIterator<Item = &'a [Phoneme]>,
    compare: Compare,
) -> Box<[&'a [Phoneme]]> {
    let mut seen = HashSet::new();

    pronunciations
        .into_iter()
        .map(|pronunciation| comparison_part(pronunciation, compare))
        .filter(|part| seen.insert(*part))
        .collect()
}

impl<D> Analyzer<VowelHexGraph, AnalyticConsonantDistance, D>
where
    D: PronouncingDictionary,
{
    pub fn vowel_distance_config(&self) -> VowelHexGraphConfig {
        self.rhyme_distance_scorer.vowel_config()
    }

    pub fn set_vowel_distance_config(&mut self, config: VowelHexGraphConfig) {
        self.rhyme_distance_scorer.set_vowel_config(config);
    }

    pub fn consonant_distance_config(&self) -> AnalyticConsonantDistanceConfig {
        self.rhyme_distance_scorer.consonant_config()
    }

    pub fn set_consonant_distance_config(&mut self, config: AnalyticConsonantDistanceConfig) {
        self.rhyme_distance_scorer.set_consonant_config(config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static ANALYZER: OnceLock<Analyzer> = OnceLock::new();

    fn analyzer() -> &'static Analyzer {
        ANALYZER.get_or_init(|| {
            let dict = CmuDictionary::load(DICT_PATH).expect("failed to load dictionary");
            Analyzer::new(dict, RhymeDistanceScorer::new())
        })
    }

    #[test]
    fn analyzer_sets_distance_configs() {
        let dict = CmuDictionary::load(DICT_PATH).expect("failed to load dictionary");
        let mut analyzer: Analyzer = Analyzer::new(dict, RhymeDistanceScorer::new());
        let mut rhyme_config = analyzer.rhyme_distance_scorer().config();
        rhyme_config.vowel_coefficient += 1;
        let mut vowel_config = analyzer.vowel_distance_config();
        vowel_config.vowel_stress_penalty += 1;
        let mut consonant_config = analyzer.consonant_distance_config();
        consonant_config.voiced_penalty += 1;

        analyzer.set_rhyme_distance_config(rhyme_config);
        analyzer.set_vowel_distance_config(vowel_config);
        analyzer.set_consonant_distance_config(consonant_config);

        assert_eq!(analyzer.rhyme_distance_scorer().config(), rhyme_config);
        assert_eq!(analyzer.vowel_distance_config(), vowel_config);
        assert_eq!(analyzer.consonant_distance_config(), consonant_config);
    }

    fn phonemes(arpa: &[&str]) -> Box<[Phoneme]> {
        arpa.iter()
            .map(|phoneme| phoneme.parse().unwrap())
            .collect()
    }

    fn target(word: &str, min: f32, max: f32) -> WordTarget<'_> {
        WordTarget {
            word,
            distance_range: min..=max,
        }
    }

    #[test]
    fn comparison_parts_are_borrowed_and_deduplicated_in_input_order() {
        let pronunciations = [
            phonemes(&["K", "AE1", "T"]),
            phonemes(&["B", "AE1", "T"]),
            phonemes(&["K", "AA1", "T"]),
        ];

        let parts = get_comparison_parts(
            pronunciations
                .iter()
                .map(|pronunciation| pronunciation.as_ref()),
            Compare::RhymingPart,
        );

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], &pronunciations[0][1..]);
        assert_eq!(parts[1], &pronunciations[2][1..]);
    }

    #[test]
    fn pronunciation_targets_can_borrow_caller_owned_phonemes() {
        let pronunciation = phonemes(&["K", "AE1", "T"]);

        // The target descriptors are local to this block; returned matches
        // borrow the caller-owned pronunciation, not the descriptor array.
        let matches = {
            let targets = [PronunciationsTarget::new(
                [pronunciation.as_ref()],
                0.0..=0.0,
            )];
            analyzer().find_rhymes_intersection_by_pronunciations(
                &targets,
                None,
                RhymeScoreNormalization::None,
                Compare::WholeWord,
            )
        };

        let cat = matches
            .iter()
            .find(|m| m.word == "cat")
            .expect("the caller-owned CAT pronunciation should match CAT");
        assert_eq!(
            cat.scores.best_subject_pronunciation,
            pronunciation.as_ref()
        );
        assert_eq!(cat.scores.best_scored_targets.len(), 1);
        assert_eq!(cat.scores.best_scored_targets[0].0, pronunciation.as_ref());
        assert_eq!(cat.scores.best_scored_targets[0].1.raw_distance, 0);
    }

    #[test]
    fn rhyming_part_comparison_projects_both_subject_and_target() {
        let cat = phonemes(&["K", "AE1", "T"]);
        let targets = [PronunciationsTarget::new([cat.as_ref()], 0.0..=0.0)];

        let matches = analyzer().find_rhymes_intersection_by_pronunciations(
            &targets,
            None,
            RhymeScoreNormalization::None,
            Compare::RhymingPart,
        );

        let bat = matches
            .iter()
            .find(|m| m.word == "bat")
            .expect("BAT and CAT should have identical rhyming parts");
        assert_eq!(bat.scores.best_subject_pronunciation[0].to_string(), "B");
        assert_eq!(bat.scores.best_scored_targets[0].0, &cat[1..]);
        assert_eq!(bat.scores.best_scored_targets[0].1.raw_distance, 0);
        assert_eq!(bat.scores.best_scored_targets[0].1.leader_len, 2);
        assert_eq!(bat.scores.best_scored_targets[0].1.target_len, 2);
    }

    #[test]
    fn intersection_uses_one_subject_pronunciation_for_every_target() {
        let a = analyzer();
        let cot = phonemes(&["K", "AA1", "T"]);
        let alternate = phonemes(&["K", "AO1", "T"]);

        // CAUGHT has K AA1 T and K AO1 T. Each pronunciation exactly matches
        // one target, but neither exactly matches both.
        for target_pronunciation in [cot.as_ref(), alternate.as_ref()] {
            let targets = [PronunciationsTarget::new([target_pronunciation], 0.0..=0.0)];
            let matches = a.find_rhymes_intersection_by_pronunciations(
                &targets,
                None,
                RhymeScoreNormalization::None,
                Compare::WholeWord,
            );
            assert!(matches.iter().any(|m| m.word == "caught"));
        }

        let targets = [
            PronunciationsTarget::new([cot.as_ref()], 0.0..=0.0),
            PronunciationsTarget::new([alternate.as_ref()], 0.0..=0.0),
        ];
        let intersection = a.find_rhymes_intersection_by_pronunciations(
            &targets,
            None,
            RhymeScoreNormalization::None,
            Compare::WholeWord,
        );
        assert!(intersection.iter().all(|m| m.word != "caught"));
    }

    #[test]
    fn intersection_returns_one_tuple_per_target_and_sorts_by_their_sum() {
        let normalization = RhymeScoreNormalization::PerMeanPhoneme;
        let matches = analyzer()
            .find_rhymes_intersection_by_words(
                &[
                    target("cat", 0.0, f32::INFINITY),
                    target("dog", 0.0, f32::INFINITY),
                ],
                normalization,
                Compare::WholeWord,
            )
            .unwrap();

        assert!(!matches.is_empty());
        assert!(
            matches
                .iter()
                .all(|m| m.scores.best_scored_targets.len() == 2)
        );
        assert!(matches.iter().all(|m| m.word != "cat" && m.word != "dog"));
        assert_eq!(matches.len(), analyzer().dict().iter().count() - 2);

        let total = |m: &RhymeIntersectionMatchOwned| -> f32 {
            m.scores
                .best_scored_targets
                .iter()
                .map(|(_, score)| score.normalized(normalization))
                .sum()
        };
        assert!(
            matches
                .windows(2)
                .all(|pair| total(&pair[0]) <= total(&pair[1]))
        );
    }

    #[test]
    fn intersection_handles_empty_and_unknown_targets() {
        let empty = analyzer()
            .find_rhymes_intersection_by_words(
                &[],
                RhymeScoreNormalization::PerMeanPhoneme,
                Compare::WholeWord,
            )
            .unwrap();
        assert!(empty.is_empty());

        let unknown = analyzer().find_rhymes_intersection_by_words(
            &[target("cat", 0.0, 1000.0), target("xyzzy", 0.0, 1000.0)],
            RhymeScoreNormalization::PerMeanPhoneme,
            Compare::WholeWord,
        );
        assert!(matches!(unknown, Err(UnknownWordError { .. })));
    }

    #[test]
    fn lower_level_intersection_and_consistent_score_handle_empty_targets() {
        let matches = analyzer().find_rhymes_intersection_by_pronunciations(
            &[],
            None,
            RhymeScoreNormalization::PerMeanPhoneme,
            Compare::WholeWord,
        );
        assert!(matches.is_empty());

        let score = analyzer()
            .calculate_consistent_score_between_words(
                "xyzzy",
                &[],
                RhymeScoreNormalization::PerMeanPhoneme,
                Compare::WholeWord,
            )
            .unwrap();
        assert!(score.is_none());
    }

    #[test]
    #[should_panic(expected = "must contain at least one pronunciation")]
    fn pronunciation_target_rejects_an_empty_pronunciation_set() {
        let pronunciations: [&[Phoneme]; 0] = [];
        PronunciationsTarget::new(pronunciations, 0.0..=f32::INFINITY);
    }

    #[test]
    #[should_panic(expected = "must contain at least one syllable")]
    fn pronunciation_target_rejects_a_zero_syllable_pronunciation() {
        let pronunciation = phonemes(&["K", "T"]);
        PronunciationsTarget::new([pronunciation.as_ref()], 0.0..=f32::INFINITY);
    }

    #[test]
    fn consistent_score_returns_owned_tuple_provenance() {
        let normalization = RhymeScoreNormalization::PerMeanPhoneme;
        let consistent = analyzer()
            .calculate_consistent_score_between_words(
                "caught",
                &["cot"],
                normalization,
                Compare::WholeWord,
            )
            .unwrap()
            .unwrap();

        assert_eq!(consistent.best_scored_targets.len(), 1);
        let (target_pronunciation, score) = &consistent.best_scored_targets[0];
        assert_eq!(score.raw_distance, 0);
        assert_eq!(score.normalized(normalization), 0.0);
        assert_eq!(consistent.best_subject_pronunciation[1].to_string(), "AA1");
        assert_eq!(target_pronunciation[1].to_string(), "AA1");
    }

    #[test]
    fn consistent_score_commits_to_one_subject_pronunciation() {
        let independent: f32 = ["voght", "fouhy"]
            .iter()
            .map(|&target| {
                analyzer()
                    .calculate_consistent_score_between_words(
                        "nikes",
                        &[target],
                        RhymeScoreNormalization::None,
                        Compare::WholeWord,
                    )
                    .unwrap()
                    .unwrap()
                    .best_scored_targets[0]
                    .1
                    .normalized(RhymeScoreNormalization::None)
            })
            .sum();

        let consistent = analyzer()
            .calculate_consistent_score_between_words(
                "nikes",
                &["voght", "fouhy"],
                RhymeScoreNormalization::None,
                Compare::WholeWord,
            )
            .unwrap()
            .unwrap();
        let total: f32 = consistent
            .best_scored_targets
            .iter()
            .map(|(_, score)| score.normalized(RhymeScoreNormalization::None))
            .sum();

        assert_eq!(consistent.best_scored_targets.len(), 2);
        assert!(total > independent);
    }

    #[test]
    fn consistent_score_rejects_unknown_words() {
        for (subject, target) in [("xyzzy", "cat"), ("cat", "xyzzy")] {
            let result = analyzer().calculate_consistent_score_between_words(
                subject,
                &[target],
                RhymeScoreNormalization::PerMeanPhoneme,
                Compare::WholeWord,
            );
            assert!(matches!(result, Err(UnknownWordError { .. })));
        }
    }

    #[test]
    fn word_scoring_and_scanning_honor_pronunciation_mode() {
        use crate::PronunciationMode;

        let dict = CmuDictionary::from_bytes(b"KETTLE  K EH1 T AH0 L\n").unwrap();
        let strict = Analyzer::new(&dict, RhymeDistanceScorer::new());
        for (subject, target_word) in [("kettle's", "kettle"), ("kettle", "kettles")] {
            assert!(
                strict
                    .calculate_consistent_score_between_words(
                        subject,
                        &[target_word],
                        RhymeScoreNormalization::None,
                        Compare::WholeWord,
                    )
                    .is_err()
            );
        }
        let targets = [target("kettles", 0.0, f32::INFINITY)];
        assert!(
            strict
                .find_rhymes_intersection_by_words(
                    &targets,
                    RhymeScoreNormalization::None,
                    Compare::WholeWord,
                )
                .is_err()
        );

        let permissive = Analyzer::new(
            (&dict).with_pronunciation_mode(PronunciationMode::AllowDerived),
            RhymeDistanceScorer::new(),
        );
        let score = permissive
            .calculate_consistent_score_between_words(
                "kettle's",
                &["kettles"],
                RhymeScoreNormalization::None,
                Compare::WholeWord,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            score.best_subject_pronunciation,
            score.best_scored_targets[0].0
        );
        assert_eq!(score.best_scored_targets[0].1.raw_distance, 0);

        let matches = permissive
            .find_rhymes_intersection_by_words(
                &targets,
                RhymeScoreNormalization::None,
                Compare::WholeWord,
            )
            .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].word, "kettle");
        assert_eq!(
            matches[0].scores.best_scored_targets[0].0,
            score.best_subject_pronunciation
        );
    }
}
