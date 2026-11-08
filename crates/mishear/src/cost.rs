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
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            vowel_substitution: 1.0,
            consonant_substitution: 1.0,
            stressed_vowel_indel: 1.0,
            unstressed_vowel_indel: 0.5,
            consonant_indel: 0.6,
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
                        config.vowel_substitution * vowels.distance(&x, &y) as f32 / max
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
        }
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

    /// The edit distance between two sequences.
    pub fn distance(&self, a: &[Ph], b: &[Ph]) -> f32 {
        let mut prev: Vec<f32> = std::iter::once(0.0)
            .chain(b.iter().scan(0.0, |acc, &q| {
                *acc += self.indel(q);
                Some(*acc)
            }))
            .collect();
        let mut row = vec![0.0; b.len() + 1];
        for &p in a {
            row[0] = prev[0] + self.indel(p);
            for (j, &q) in b.iter().enumerate() {
                row[j + 1] = (prev[j] + self.sub(p, q))
                    .min(prev[j + 1] + self.indel(p))
                    .min(row[j] + self.indel(q));
            }
            std::mem::swap(&mut prev, &mut row);
        }
        prev[b.len()]
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
