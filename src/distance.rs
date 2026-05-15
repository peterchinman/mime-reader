use std::collections::HashMap;

use crate::phoneme::Phoneme;
use crate::vowel_distance::VowelHexGraph;

// We never want to compare a vowel to a consonant, so we can give this an arbitrarily high penalty.
const VOWEL_TO_CONSONANT_MISMATCH: u32 = 100;
// Vowel distances get multiplied by this coefficient.
// Vowel distance ranges from 0-9 (VowelHexGraph distance up to 4, gets multiplied by VOWEL_DISTANCE_COEFFICIENT, and VOWEL_STRESS_PENALTY added), consonant distance range up to 10.
// This coefficient determines "how much more important vowels are than consonants"
const VOWEL_COEFFICIENT: u32 = 2;
// Insertion/deletion penalty for vowels
const VOWEL_INDEL_PENALTY: u32 = 20;
// Insertion/deletion penalty for consonants
// This should be greater or equal to half of the Consonant's UNRELATED_PENALTY, otherwise it's cheaper to insert/delete a consonant than to compare consonants.
const CONSONANT_INDEL_PENALTY: u32 = 5;
const CONSONANT_REPEATED_PENALTY: u32 = 1;
const TRANSPOSITION_COST: u32 = 2;

const INF: u32 = u32::MAX / 2;

pub struct DamerauLevenshtein {
    vowel_graph: VowelHexGraph,
}

impl DamerauLevenshtein {
    pub fn new() -> Self {
        Self {
            vowel_graph: VowelHexGraph::new(),
        }
    }

    fn gap_penalty(phoneme: &Phoneme, prev: Option<&Phoneme>) -> u32 {
        match phoneme {
            Phoneme::Vowel(_) => VOWEL_INDEL_PENALTY,
            Phoneme::Consonant(c) => {
                if let Some(Phoneme::Consonant(p)) = prev {
                    if c.phoneme == p.phoneme {
                        return CONSONANT_REPEATED_PENALTY;
                    }
                }
                CONSONANT_INDEL_PENALTY
            }
        }
    }

    fn substitution_score(&self, p1: &Phoneme, p2: &Phoneme) -> u32 {
        match (p1, p2) {
            (Phoneme::Vowel(v1), Phoneme::Vowel(v2)) => {
                v1.distance(v2, &self.vowel_graph) * VOWEL_COEFFICIENT
            }
            (Phoneme::Consonant(c1), Phoneme::Consonant(c2)) => c1.distance(c2),
            _ => VOWEL_TO_CONSONANT_MISMATCH,
        }
    }

    // first_prev: the phoneme just before slice[0] in the original sequence (stripped prefix context).
    fn prefix_gap_sums(slice: &[Phoneme], first_prev: Option<&Phoneme>) -> Vec<u32> {
        let mut sums = vec![0u32; slice.len() + 1];
        for i in 0..slice.len() {
            let prev = if i > 0 {
                Some(&slice[i - 1])
            } else {
                first_prev
            };
            sums[i + 1] = sums[i] + Self::gap_penalty(&slice[i], prev);
        }
        sums
    }

    pub fn distance(&self, slice1: &[Phoneme], slice2: &[Phoneme]) -> u32 {
        // Strip common prefix
        let prefix = slice1
            .iter()
            .zip(slice2.iter())
            .take_while(|(a, b)| a == b)
            .count();
        // The element just before the stripped region — needed for repeated-consonant gap detection.
        let prefix_context: Option<&Phoneme> = if prefix > 0 {
            Some(&slice1[prefix - 1])
        } else {
            None
        };
        let s1 = &slice1[prefix..];
        let s2 = &slice2[prefix..];

        // Strip common suffix
        let suffix = s1
            .iter()
            .rev()
            .zip(s2.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let s1 = &s1[..s1.len() - suffix];
        let s2 = &s2[..s2.len() - suffix];

        // Ensure s1 is the shorter slice
        let (s1, s2) = if s2.len() < s1.len() {
            (s2, s1)
        } else {
            (s1, s2)
        };

        let len1 = s1.len();
        let len2 = s2.len();

        let prefix_gap1 = Self::prefix_gap_sums(s1, prefix_context);
        let prefix_gap2 = Self::prefix_gap_sums(s2, prefix_context);

        if len1 == 0 {
            return prefix_gap2[len2];
        }

        // DP matrix: (len1+2) rows × (len2+2) cols
        // Row/col 0 are INF sentinels; row/col 1 are base cases.
        let rows = len1 + 2;
        let cols = len2 + 2;
        let mut dists = vec![vec![INF; cols]; rows];

        dists[1][1] = 0;
        for i in 1..=len1 {
            dists[i + 1][1] = prefix_gap1[i];
        }
        for j in 1..=len2 {
            dists[1][j + 1] = prefix_gap2[j];
        }

        let mut last_i1: HashMap<Phoneme, usize> = HashMap::new();

        for (i1, p1) in s1.iter().enumerate() {
            let mut l2 = 0usize;

            for (i2, p2) in s2.iter().enumerate() {
                let l1 = *last_i1.get(p2).unwrap_or(&0);

                let del_cost = Self::gap_penalty(
                    p1,
                    if i1 > 0 {
                        s1.get(i1 - 1)
                    } else {
                        prefix_context
                    },
                );
                let ins_cost = Self::gap_penalty(
                    p2,
                    if i2 > 0 {
                        s2.get(i2 - 1)
                    } else {
                        prefix_context
                    },
                );
                let sub_cost = self.substitution_score(p1, p2);
                let trans_gap1 = prefix_gap1[i1] - prefix_gap1[l1];
                let trans_gap2 = prefix_gap2[i2] - prefix_gap2[l2];
                let trans_cost = dists[l1][l2]
                    .saturating_add(trans_gap1)
                    .saturating_add(trans_gap2)
                    .saturating_add(TRANSPOSITION_COST);

                dists[i1 + 2][i2 + 2] = [
                    dists[i1 + 2][i2 + 1].saturating_add(del_cost),
                    dists[i1 + 1][i2 + 2].saturating_add(ins_cost),
                    dists[i1 + 1][i2 + 1].saturating_add(sub_cost),
                    trans_cost,
                ]
                .into_iter()
                .min()
                .unwrap();

                if p1 == p2 {
                    l2 = i2 + 1;
                }
            }

            last_i1.insert(p1.clone(), i1 + 1);
        }

        dists[len1 + 1][len2 + 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vowel_distance::VOWEL_DISTANCE_COEFFICIENT;
    use std::sync::OnceLock;

    static DL: OnceLock<DamerauLevenshtein> = OnceLock::new();
    fn dl() -> &'static DamerauLevenshtein {
        DL.get_or_init(DamerauLevenshtein::new)
    }

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    #[test]
    fn equal_slices_are_zero() {
        let s = parse("M AO1 S T");
        assert_eq!(dl().distance(&s, &s), 0);
    }

    #[test]
    fn empty_vs_consonants() {
        let s = parse("M AO1 S T");
        // consonant gap = 5 each, vowel gap = 20
        // M(5) + AO1(20) + S(5) + T(5) = 35
        assert_eq!(dl().distance(&[], &s), 35);
        assert_eq!(dl().distance(&s, &[]), 35);
    }

    #[test]
    fn both_empty() {
        assert_eq!(dl().distance(&[], &[]), 0);
    }

    #[test]
    fn adjacent_vowel_substitution() {
        // AO1 vs OW1: adjacent on hex-graph (dist=1), same stress
        // cost = (1 * VOWEL_DISTANCE_COEFFICIENT + 0) * VOWEL_COEFFICIENT
        let s1 = parse("M AO1 S T");
        let s2 = parse("M OW1 S T");
        assert_eq!(dl().distance(&s1, &s2), 1 * VOWEL_DISTANCE_COEFFICIENT * VOWEL_COEFFICIENT);
    }

    #[test]
    fn consonant_substitution_voiced_pair() {
        // T vs D: same place (4), voiced penalty only
        let s1 = parse("T AE1 N");
        let s2 = parse("D AE1 N");
        // T vs D: same manner (Plosive), place diff 0, voiced penalty 1
        assert_eq!(dl().distance(&s1, &s2), 1);
    }

    #[test]
    fn transposition_of_adjacent_consonants() {
        // "M AO1 S T" vs "M AO1 T S": last two consonants transposed
        // The two elements between last occurrences of each in the other are empty,
        // so cost = TRANSPOSITION_COST
        let s1 = parse("M AO1 S T");
        let s2 = parse("M AO1 T S");
        assert_eq!(dl().distance(&s1, &s2), TRANSPOSITION_COST);
    }

    #[test]
    fn single_consonant_insertion() {
        let s1 = parse("M AO1 T");
        let s2 = parse("M AO1 S T");
        assert_eq!(dl().distance(&s1, &s2), CONSONANT_INDEL_PENALTY);
    }

    #[test]
    fn repeated_consonant_low_penalty() {
        // Inserting a consonant identical to its neighbour costs CONSONANT_REPEATED_PENALTY
        let s1 = parse("B AE1 T");
        let s2 = parse("B AE1 T T");
        assert_eq!(dl().distance(&s1, &s2), CONSONANT_REPEATED_PENALTY);
    }

    #[test]
    fn distance_is_symmetric() {
        let s1 = parse("M AO1 S T");
        let s2 = parse("B OW1 T");
        assert_eq!(dl().distance(&s1, &s2), dl().distance(&s2, &s1));
    }
}
