//! Echo's pair scores: phoneme similarity with calibrated zero points.
//!
//! A phoneme-distance engine says how *far apart* two phonemes are (0 =
//! identical, 1000 = the engine's most distant pair). Echo needs a
//! *similarity* that is positive for alike phonemes and negative for
//! unrelated ones, so that local and semi-global alignment can tell a shared
//! region from noise. The zero point is where a pair stops counting as
//! alike.
//!
//! The natural zero point is chance: the expected distance between two
//! phonemes drawn at random from English (type-weighted over every CMUdict
//! pronunciation), so a random pair scores 0 on average. The vowel default
//! is that calibrated value. The consonant default is tuned well below it
//! ([`TUNED_CONSONANT_ZERO_POINT`]): at chance, an alignment that may skip
//! material collects chance consonant matches, and on the labeled corpus a
//! stricter consonant zero point separated echoes from non-echoes better
//! than any other single change.

use crate::align::{NEG_INF, ScoreTable, phoneme_id};
use crate::consonant_distance::ConsonantDistanceScorer;
use crate::dictionary::PronouncingDictionary;
#[cfg(test)]
use crate::phoneme::Vowel;
use crate::phoneme::{Phoneme, Stress};
use crate::vowel_distance::VowelDistanceScorer;

/// The expected [`crate::VowelHexGraph`] distance between two vowels drawn
/// at random from CMUdict (stress included), under the default vowel config.
/// The default vowel zero point. Recomputed by `zero_points_match_cmudict`.
pub const CALIBRATED_VOWEL_ZERO_POINT: u32 = 376;
/// The expected [`crate::AnalyticConsonantDistance`] distance between two
/// consonants drawn at random from CMUdict, under the default consonant
/// config. A measurement, not the default (see
/// [`TUNED_CONSONANT_ZERO_POINT`]). Recomputed by `zero_points_match_cmudict`.
pub const CALIBRATED_CONSONANT_ZERO_POINT: u32 = 789;
/// The default consonant zero point, tuned on the labeled echo corpus
/// (`data/echo/README.md`). Every aligner's search preferred 300–540 to the
/// calibrated 789, and this value alone recovered most of semi-global's
/// tuning gain. With it most random consonant pairs score below 0, and an
/// identical consonant is worth about a quarter of an identical primary
/// vowel (at the default coefficients).
pub const TUNED_CONSONANT_ZERO_POINT: u32 = 300;

/// How much each vowel stress counts. Consonants are weighted flat, at
/// `primary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StressWeights {
    pub primary: u32,
    pub secondary: u32,
    pub unstressed: u32,
}

impl Default for StressWeights {
    fn default() -> Self {
        Self {
            primary: 4,
            secondary: 3,
            unstressed: 2,
        }
    }
}

impl StressWeights {
    pub fn weight(&self, stress: Stress) -> u32 {
        match stress {
            Stress::Primary => self.primary,
            Stress::Secondary => self.secondary,
            Stress::Unstressed => self.unstressed,
        }
    }
}

/// Echo's own similarity coefficients. These are deliberately independent
/// of [`crate::RhymeDistanceConfig`]: its 7:1 vowel:consonant ratio was tuned
/// to separate kinds of rhyme, not to measure whole-line repetition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarityConfig {
    pub vowel_coefficient: u32,
    pub consonant_coefficient: u32,
    /// Vowel distance (0-1000) at which a vowel pair scores 0.
    pub vowel_zero_point: u32,
    /// Consonant distance (0-1000) at which a consonant pair scores 0.
    pub consonant_zero_point: u32,
    pub stress_weights: StressWeights,
}

/// Coefficients 3:1, the calibrated vowel zero point and the tuned consonant
/// zero point.
impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            vowel_coefficient: 3,
            consonant_coefficient: 1,
            vowel_zero_point: CALIBRATED_VOWEL_ZERO_POINT,
            consonant_zero_point: TUNED_CONSONANT_ZERO_POINT,
            stress_weights: StressWeights::default(),
        }
    }
}

/// Builds echo's [`ScoreTable`] and its `unit`, the similarity of an
/// identical primary-stressed vowel (`vowel_coefficient × vowel_zero_point ×
/// primary`), in which every echo cost is expressed.
///
/// - vowels: `vowel_coefficient × (vowel_zero_point − d(p, q)) × min(w(p), w(q))`.
///   Taking the *smaller* stress weight makes `sim(p, q) ≤ min(sim(p, p),
///   sim(q, q))`, so an alignment never scores above either line's
///   self-similarity, and a verbatim repeat normalizes to exactly 1.
/// - consonants: `consonant_coefficient × (consonant_zero_point − d(p, q)) ×
///   primary` — stress doesn't apply.
/// - a vowel against a consonant: [`NEG_INF`], never aligned.
pub fn similarity_table<V, C>(
    config: &SimilarityConfig,
    vowels: &V,
    consonants: &C,
) -> (ScoreTable, i64)
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    let weights = config.stress_weights;
    let table = ScoreTable::from_fn(|p, q| match (p, q) {
        (Phoneme::Vowel(v1), Phoneme::Vowel(v2)) => {
            let weight = weights.weight(v1.stress).min(weights.weight(v2.stress));
            i64::from(config.vowel_coefficient)
                * (i64::from(config.vowel_zero_point) - i64::from(vowels.distance(v1, v2)))
                * i64::from(weight)
        }
        (Phoneme::Consonant(c1), Phoneme::Consonant(c2)) => {
            i64::from(config.consonant_coefficient)
                * (i64::from(config.consonant_zero_point) - i64::from(consonants.distance(c1, c2)))
                * i64::from(weights.primary)
        }
        _ => NEG_INF,
    });
    let unit = i64::from(config.vowel_coefficient)
        * i64::from(config.vowel_zero_point)
        * i64::from(weights.primary);
    (table, unit)
}

/// Zero points calibrated against a dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroPoints {
    pub vowel: u32,
    pub consonant: u32,
}

/// The expected engine distance between two vowels, and between two
/// consonants, drawn independently from `dict`'s phoneme frequencies: each
/// phoneme type (vowels with their stress) weighted by how often it occurs
/// across every pronunciation, `E = Σ n_p n_q d(p, q) / N²`, rounded.
pub fn calibrate_zero_points<D, V, C>(dict: &D, vowels: &V, consonants: &C) -> ZeroPoints
where
    D: PronouncingDictionary,
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    let mut counts = [0u128; crate::align::PHONEME_IDS];
    for (_, pronunciations) in dict.entries() {
        for pronunciation in pronunciations {
            for phoneme in pronunciation.iter() {
                counts[usize::from(phoneme_id(phoneme))] += 1;
            }
        }
    }
    let phonemes = crate::align::all_phonemes();
    let expected = |pick: &dyn Fn(&Phoneme, &Phoneme) -> Option<u32>| -> u32 {
        let (mut total, mut weight) = (0u128, 0u128);
        for (p, &n_p) in phonemes.iter().zip(&counts) {
            for (q, &n_q) in phonemes.iter().zip(&counts) {
                if let Some(distance) = pick(p, q) {
                    total += n_p * n_q * u128::from(distance);
                    weight += n_p * n_q;
                }
            }
        }
        if weight == 0 {
            0
        } else {
            ((total + weight / 2) / weight) as u32
        }
    };
    ZeroPoints {
        vowel: expected(&|p, q| match (p, q) {
            (Phoneme::Vowel(v1), Phoneme::Vowel(v2)) => Some(vowels.distance(v1, v2)),
            _ => None,
        }),
        consonant: expected(&|p, q| match (p, q) {
            (Phoneme::Consonant(c1), Phoneme::Consonant(c2)) => Some(consonants.distance(c1, c2)),
            _ => None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo::test_support::{dict, parse};
    use crate::{AnalyticConsonantDistance, VowelHexGraph};

    fn p(s: &str) -> Phoneme {
        s.parse().unwrap()
    }

    fn default_table() -> (ScoreTable, i64) {
        similarity_table(
            &SimilarityConfig::default(),
            &VowelHexGraph::new(),
            &AnalyticConsonantDistance::new(),
        )
    }

    #[test]
    fn zero_points_match_cmudict() {
        let calibrated = calibrate_zero_points(
            dict(),
            &VowelHexGraph::new(),
            &AnalyticConsonantDistance::new(),
        );
        assert_eq!(
            calibrated,
            ZeroPoints {
                vowel: CALIBRATED_VOWEL_ZERO_POINT,
                consonant: CALIBRATED_CONSONANT_ZERO_POINT,
            },
            "re-pin the zero-point constants to the calibrated values",
        );
    }

    #[test]
    fn identical_vowels_score_by_stress() {
        let (table, unit) = default_table();
        assert!(unit > 0);
        assert_eq!(table.score(&p("AE1"), &p("AE1")), unit);
        assert_eq!(table.score(&p("AE2"), &p("AE2")) * 4, unit * 3);
        assert_eq!(table.score(&p("AE0"), &p("AE0")) * 2, unit);
    }

    #[test]
    fn a_pair_never_beats_either_self_score() {
        let (table, _) = default_table();
        let phonemes = crate::align::all_phonemes();
        for x in &phonemes {
            for y in &phonemes {
                let pair = table.score(x, y);
                assert_eq!(pair, table.score(y, x), "{x} {y} symmetric");
                if matches!(
                    (x, y),
                    (Phoneme::Vowel(_), Phoneme::Vowel(_))
                        | (Phoneme::Consonant(_), Phoneme::Consonant(_))
                ) {
                    assert!(pair <= table.score(x, x).min(table.score(y, y)), "{x} {y}");
                } else {
                    assert_eq!(pair, NEG_INF, "{x} {y} cross-class");
                }
            }
        }
    }

    #[test]
    fn mixed_stress_takes_the_smaller_weight() {
        let (table, _) = default_table();
        let config = SimilarityConfig::default();
        let distance = VowelHexGraph::new().distance(
            &"AE1".parse::<Vowel>().unwrap(),
            &"AE0".parse::<Vowel>().unwrap(),
        );
        assert_eq!(
            table.score(&p("AE1"), &p("AE0")),
            i64::from(config.vowel_coefficient)
                * (i64::from(config.vowel_zero_point) - i64::from(distance))
                * i64::from(config.stress_weights.unstressed)
        );
    }

    #[test]
    fn consonants_are_flat_and_unrelated_ones_negative() {
        let (table, unit) = default_table();
        let config = SimilarityConfig::default();
        assert_eq!(
            table.score(&p("T"), &p("T")),
            i64::from(config.consonant_coefficient)
                * i64::from(config.consonant_zero_point)
                * i64::from(config.stress_weights.primary)
        );
        // With the tuned consonant zero point, an identical consonant is
        // worth about a quarter of an identical primary vowel.
        let ratio = table.score(&p("T"), &p("T")) as f64 / unit as f64;
        assert!(
            (0.2..0.35).contains(&ratio),
            "consonant/vowel ratio {ratio}"
        );
        assert!(table.score(&p("M"), &p("F")) < 0);
        assert!(
            table.score(&p("Z"), &p("S")) > 0,
            "voicing pairs stay alike"
        );
        let line = parse("D AA1 R K");
        assert!(line.iter().all(|x| table.score(x, x) > 0));
    }
}
