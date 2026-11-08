//! How different two phoneme sequences sound: a weighted edit distance whose
//! substitution costs come from mime-reader's vowel and consonant distance
//! engines (the ones rhyme and echo use), on a 0–1 scale per phoneme.

use mime_reader::consonant_distance::ConsonantDistanceScorer;
use mime_reader::{AnalyticConsonantDistance, NORMALIZED_DISTANCE_MAX, Phoneme, VowelHexGraph};

use crate::phone::Ph;

/// The weights of the edit distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostConfig {
    /// Substituting the farthest-apart vowels.
    pub vowel_substitution: f32,
    /// Substituting the farthest-apart consonants.
    pub consonant_substitution: f32,
    /// Adding or dropping a stressed vowel (a syllable).
    pub stressed_vowel_indel: f32,
    /// Adding or dropping an unstressed vowel (a schwa, mostly).
    pub unstressed_vowel_indel: f32,
    /// Adding or dropping a consonant.
    pub consonant_indel: f32,
    /// Extra for pairing a stressed vowel with an unstressed one, on top of
    /// the vowel engine's own small stress penalty (0.04): how much a hearing
    /// must keep the line's rhythm.
    pub stress_mismatch: f32,
    /// Exchanging two adjacent, different consonants ("ask" ↔ "aks"), as
    /// one edit at this cost; `None` disallows it.
    pub swap: Option<f32>,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            vowel_substitution: 1.0,
            consonant_substitution: 1.0,
            stressed_vowel_indel: 1.0,
            unstressed_vowel_indel: 0.5,
            consonant_indel: 0.6,
            stress_mismatch: 0.0,
            swap: None,
        }
    }
}

/// Edit costs for every phoneme pair. A vowel never substitutes for a
/// consonant (it would cost more than dropping one and adding the other).
#[derive(Clone)]
pub struct CostModel {
    sub: Vec<f32>,
    indel: Vec<f32>,
    min_indel: f32,
    swap: Option<f32>,
}

impl CostModel {
    pub fn new(config: CostConfig) -> Self {
        let vowels = VowelHexGraph::new();
        let consonants = AnalyticConsonantDistance::new();
        let max = NORMALIZED_DISTANCE_MAX as f32;
        let n = Ph::COUNT;
        let mut sub = vec![f32::INFINITY; n * n];
        for a in Ph::all() {
            for b in Ph::all() {
                let cost = match (a.to_phoneme(), b.to_phoneme()) {
                    (Phoneme::Vowel(x), Phoneme::Vowel(y)) => {
                        let rhythm = if a.is_stressed() == b.is_stressed() {
                            0.0
                        } else {
                            config.stress_mismatch
                        };
                        config.vowel_substitution * vowels.distance(&x, &y) as f32 / max + rhythm
                    }
                    (Phoneme::Consonant(x), Phoneme::Consonant(y)) => {
                        config.consonant_substitution * consonants.distance(&x, &y) as f32 / max
                    }
                    _ => f32::INFINITY,
                };
                sub[a.index() * n + b.index()] = cost;
            }
        }
        let indel: Vec<f32> = Ph::all()
            .map(|p| match p.stress() {
                Some(0) => config.unstressed_vowel_indel,
                Some(_) => config.stressed_vowel_indel,
                None => config.consonant_indel,
            })
            .collect();
        let min_indel = indel.iter().copied().fold(f32::INFINITY, f32::min);
        Self {
            sub,
            indel,
            min_indel,
            swap: config.swap,
        }
    }

    /// The cost of reading `a b` (in that order) as `b a`, when swaps are
    /// on and `a`, `b` are different consonants.
    #[inline]
    pub fn swap(&self, a: Ph, b: Ph) -> Option<f32> {
        self.swap
            .filter(|_| a != b && !a.is_vowel() && !b.is_vowel())
    }

    /// The swap cost when swaps are on (a lower bound on any swap).
    pub fn swap_cost(&self) -> Option<f32> {
        self.swap
    }

    #[inline]
    pub fn sub(&self, a: Ph, b: Ph) -> f32 {
        self.sub[a.index() * Ph::COUNT + b.index()]
    }

    /// Adding or dropping `p`.
    #[inline]
    pub fn indel(&self, p: Ph) -> f32 {
        self.indel[p.index()]
    }

    pub fn min_indel(&self) -> f32 {
        self.min_indel
    }

    /// The edit distance between two sequences (optimal string alignment
    /// when swaps are on: no phoneme is edited twice).
    pub fn distance(&self, a: &[Ph], b: &[Ph]) -> f32 {
        let (n, m) = (a.len(), b.len());
        let mut d = vec![vec![0.0f32; m + 1]; n + 1];
        for j in 1..=m {
            d[0][j] = d[0][j - 1] + self.indel(b[j - 1]);
        }
        for i in 1..=n {
            d[i][0] = d[i - 1][0] + self.indel(a[i - 1]);
            for j in 1..=m {
                let mut best = (d[i - 1][j - 1] + self.sub(a[i - 1], b[j - 1]))
                    .min(d[i - 1][j] + self.indel(a[i - 1]))
                    .min(d[i][j - 1] + self.indel(b[j - 1]));
                if i >= 2
                    && j >= 2
                    && a[i - 1] == b[j - 2]
                    && a[i - 2] == b[j - 1]
                    && let Some(swap) = self.swap(a[i - 2], a[i - 1])
                {
                    best = best.min(d[i - 2][j - 2] + swap);
                }
                d[i][j] = best;
            }
        }
        d[n][m]
    }
}

impl Default for CostModel {
    fn default() -> Self {
        Self::new(CostConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ph(s: &str) -> Vec<Ph> {
        Ph::parse_all(s).unwrap()
    }

    #[test]
    fn identical_is_free_and_near_is_cheap() {
        let c = CostModel::default();
        assert_eq!(c.distance(&ph("K IH1 S"), &ph("K IH1 S")), 0.0);
        let near = c.distance(&ph("DH AH0 S K AY1"), &ph("DH IH1 S G AY1"));
        let far = c.distance(&ph("DH AH0 S K AY1"), &ph("M OW1 N"));
        assert!(near < 0.6 && far > 2.0, "near {near}, far {far}");
        assert!(
            c.sub(Ph::parse("AE1").unwrap(), Ph::parse("T").unwrap())
                .is_infinite()
        );
    }
}

#[cfg(test)]
mod knob_tests {
    use super::*;

    fn ph(s: &str) -> Vec<Ph> {
        Ph::parse_all(s).unwrap()
    }

    #[test]
    fn swaps_and_rhythm_are_opt_in() {
        let plain = CostModel::default();
        let knobs = CostModel::new(CostConfig {
            stress_mismatch: 0.4,
            swap: Some(0.2),
            ..CostConfig::default()
        });
        // "ask" against "aks": two edits without swaps, one with.
        let (ask, aks) = (ph("AE1 S K"), ph("AE1 K S"));
        assert!(plain.distance(&ask, &aks) > 0.2);
        assert!((knobs.distance(&ask, &aks) - 0.2).abs() < 1e-6);
        // A stressed vowel for an unstressed one costs the mismatch extra.
        let (a1, a0) = (Ph::parse("AE1").unwrap(), Ph::parse("AE0").unwrap());
        assert!((knobs.sub(a1, a0) - plain.sub(a1, a0) - 0.4).abs() < 1e-6);
        // Vowels never swap.
        assert!(
            knobs
                .swap(Ph::parse("AE1").unwrap(), Ph::parse("IY1").unwrap())
                .is_none()
        );
    }
}
