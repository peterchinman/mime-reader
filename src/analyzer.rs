//! The library's long-lived facade: a pronouncing dictionary bundled with a
//! rhyme-distance scorer, plus the dictionary-wide operations (like
//! [`Analyzer::find_rhymes`]) that need both. Poem analysis lives in
//! [`crate::poem`]; an [`Analyzer`] supplies the shared state poems borrow.

use std::ops::RangeInclusive;

use crate::{
    CmuDictionary, Phoneme, PronouncingDictionary, RhymeDistanceConfig, RhymeDistanceScorer,
    RhymeNormalization, RhymePairScore, UnknownWordError,
    consonant_distance::{
        AnalyticConsonantDistance, AnalyticConsonantDistanceConfig, ConsonantDistanceScorer,
    },
    phoneme::{RhymingPart, get_last_n_syllables, get_rhyming_part, get_syllable_count},
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

/// Which portion of the target word a dictionary scan compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    WholeWord,
    RhymingPart,
}

#[derive(Debug)]
pub struct RhymeMatch<'a> {
    pub word: &'a str,
    /// The dictionary pronunciation that produced the best score.
    pub pronunciation: &'a [Phoneme],
    pub score: RhymePairScore,
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

    /// Scans the dictionary for words with a pairing inside `distance_range`
    /// of the target, each scored by its best in-range pairing, sorted
    /// nearest first (ties broken alphabetically).
    ///
    /// Each match carries the pronunciation that produced its score, so
    /// callers can show or re-align the winning comparison. Out-of-range
    /// pairings are ignored per pairing, not per word: with a nonzero lower
    /// bound, a word one of whose pronunciations is a homophone of the target
    /// still appears, scored by its nearest non-trivial pronunciation.
    /// Passing a full range (`0.0..=f32::INFINITY`) returns the whole
    /// dictionary ranked by rhyme distance. The target word itself is
    /// excluded.
    ///
    /// TODO: this is a naive implementation that simply iterates the entire dict. There are faster implementations that would bucket the dictionary by vowel order.
    pub fn find_rhymes(
        &self,
        target: &str,
        distance_range: RangeInclusive<f32>,
        normalization: RhymeNormalization,
        compare: Compare,
    ) -> Result<Vec<RhymeMatch<'_>>, UnknownWordError> {
        let Some(target_pronunciations) = self.dict.lookup(target) else {
            return Err(UnknownWordError {
                word: target.to_string(),
            });
        };
        let target_lower = target.to_lowercase();

        // The distinct comparison parts across the target's pronunciations,
        // deduplicated: alternate pronunciations often share a rhyming part
        // (e.g. HELLO's two entries both end in OW1), and a duplicate part
        // would re-score every dictionary word for nothing.
        let mut target_parts: Vec<RhymingPart<'_>> = Vec::new();
        for pronunciation in &target_pronunciations {
            let part = match compare {
                Compare::RhymingPart => get_rhyming_part(pronunciation),
                Compare::WholeWord => RhymingPart {
                    phonemes: pronunciation,
                    syllable_count: get_syllable_count(pronunciation),
                },
            };
            if !target_parts.contains(&part) {
                target_parts.push(part);
            }
        }

        let mut matches: Vec<RhymeMatch<'_>> = Vec::new();

        for (dict_word, dict_pronunciations) in self.dict.entries() {
            if dict_word == target_lower {
                continue;
            }

            // The best in-range pairing of any pronunciation of this word
            // with any comparison part of the target. Filtering before
            // min-tracking means a lower bound suppresses trivial pairings
            // (e.g. a homophone pronunciation), not the whole word.
            let mut best: Option<RhymeMatch<'_>> = None;
            for dict_pronunciation in dict_pronunciations {
                for target_part in target_parts.iter().filter(|part| part.syllable_count > 0) {
                    let dict_part: &[Phoneme] = match compare {
                        Compare::RhymingPart => {
                            match get_last_n_syllables(
                                dict_pronunciation,
                                target_part.syllable_count,
                            ) {
                                Some(ending) => ending,
                                // This word has fewer syllables than the part.
                                None => continue,
                            }
                        }
                        Compare::WholeWord => dict_pronunciation,
                    };

                    let score = score_rhyme_pair(
                        &self.rhyme_distance_scorer,
                        normalization,
                        target_part.syllable_count,
                        target_part.phonemes,
                        dict_part,
                    );
                    if !distance_range.contains(&score.normalized_distance) {
                        continue;
                    }

                    let is_new_best = best
                        .as_ref()
                        .is_none_or(|b| score.normalized_distance < b.score.normalized_distance);
                    if is_new_best {
                        best = Some(RhymeMatch {
                            word: dict_word,
                            pronunciation: dict_pronunciation,
                            score,
                        });
                    }
                }
            }

            if let Some(best) = best {
                matches.push(best);
            }
        }

        // Alphabetical tie-break keeps equal-distance words in a stable order
        // (the pre-sort order is HashMap iteration, which is arbitrary).
        matches.sort_by(|a, b| {
            a.score
                .normalized_distance
                .total_cmp(&b.score.normalized_distance)
                .then_with(|| a.word.cmp(b.word))
        });

        Ok(matches)
    }
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

    // --- find_rhymes ---

    #[test]
    fn find_rhymes_returns_perfect_rhymes_with_provenance() {
        // Range 0..=0: only identical rhyming parts qualify.
        let matches = analyzer()
            .find_rhymes(
                "cat",
                0.0..=0.0,
                RhymeNormalization::PerMeanPhoneme,
                Compare::RhymingPart,
            )
            .unwrap();

        let hat = matches
            .iter()
            .find(|m| m.word == "hat")
            .expect("HAT (HH AE1 T) is a perfect rhyme for CAT (K AE1 T)");
        assert_eq!(hat.score.raw_distance, 0);
        // Provenance: the winning pronunciation is the full dictionary entry.
        assert_eq!(hat.pronunciation.len(), 3);

        assert!(
            matches.iter().all(|m| m.word != "cat"),
            "the target word must not rhyme with itself"
        );
        assert!(
            matches.iter().all(|m| m.score.raw_distance == 0),
            "a 0..=0 range must only admit distance-0 matches"
        );
    }

    #[test]
    fn find_rhymes_sorts_by_normalized_distance() {
        let matches = analyzer()
            .find_rhymes(
                "cat",
                0.0..=1000.0,
                RhymeNormalization::PerMeanPhoneme,
                Compare::RhymingPart,
            )
            .unwrap();

        assert!(!matches.is_empty());
        assert!(
            matches
                .windows(2)
                .all(|w| w[0].score.normalized_distance <= w[1].score.normalized_distance),
            "matches must be sorted nearest-first"
        );
        // A slant rhyme (cot: AA1 vs AE1) scores worse than a perfect rhyme
        // (hat) but is admitted by the wider range.
        let position = |word| matches.iter().position(|m| m.word == word);
        let (hat, cot) = (position("hat").unwrap(), position("cot").unwrap());
        assert!(hat < cot);
    }

    #[test]
    fn find_rhymes_lower_bound_skips_pairings_not_words() {
        // COT (K AA1 T) vs CAUGHT (K AA1 T / K AO1 T): CAUGHT's first
        // pronunciation is a homophone pairing (distance 0), which the lower
        // bound excludes — but the word still matches through K AO1 T, and
        // carries that pronunciation as provenance.
        let matches = analyzer()
            .find_rhymes(
                "cot",
                0.1..=1000.0,
                RhymeNormalization::PerMeanPhoneme,
                Compare::RhymingPart,
            )
            .unwrap();

        let caught = matches
            .iter()
            .find(|m| m.word == "caught")
            .expect("CAUGHT should match via its non-homophone pronunciation");
        assert!(caught.score.normalized_distance > 0.0);
        assert_eq!(caught.pronunciation[1].to_string(), "AO1");

        // A word whose only pairing is trivial (COTT: K AA1 T) stays excluded.
        assert!(matches.iter().all(|m| m.word != "cott"));
    }

    #[test]
    fn find_rhymes_rejects_unknown_target() {
        let result = analyzer().find_rhymes(
            "xyzzy",
            0.0..=1.0,
            RhymeNormalization::PerMeanPhoneme,
            Compare::RhymingPart,
        );

        assert!(matches!(result, Err(UnknownWordError { .. })));
    }
}
