use crate::phoneme::{Consonant, ConsonantManner::*, ConsonantPhoneme, ConsonantPhoneme::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalyticConsonantDistanceConfig {
    /// The weight of place distance relative to other consonant penalties.
    pub place_distance_coefficient: u32,
    /// Furthest related consonants are 7 apart, so unrelated consonants should be... somewhat further than that.
    pub unrelated_penalty: u32,
    /// TIN, DIN.
    pub voiced_penalty: u32,
    /// SICK, THICK.
    pub sibilance_penalty: u32,
    /// ROOT, LOOT.
    pub r_l_distance: u32,
    /// WHILE, VIAL.
    pub w_v_distance: u32,
    /// CHIN, SHIN.
    pub affricate_sibilant_fricative_penalty: u32,
    /// CHIN, TIN.
    pub affricate_plosive_penalty: u32,
    /// CHIN, THIN.
    pub affricate_non_sibilant_fricative_penalty: u32,
}

impl Default for AnalyticConsonantDistanceConfig {
    fn default() -> Self {
        Self {
            place_distance_coefficient: 1,
            unrelated_penalty: 10,
            voiced_penalty: 1,
            sibilance_penalty: 1,
            r_l_distance: 1,
            w_v_distance: 2,
            affricate_sibilant_fricative_penalty: 1,
            affricate_plosive_penalty: 2,
            affricate_non_sibilant_fricative_penalty: 2,
        }
    }
}

fn is_pair(a: &Consonant, b: &Consonant, x: ConsonantPhoneme, y: ConsonantPhoneme) -> bool {
    (a.phoneme == x && b.phoneme == y) || (a.phoneme == y && b.phoneme == x)
}

fn is_approximant(c: &Consonant) -> bool {
    matches!(c.manner, Approximant | LateralApproximant)
}

pub trait ConsonantDistanceScorer {
    fn distance(&self, c1: &Consonant, c2: &Consonant) -> u32;
}

/// Implements a consonant distance score derived from my own analysis of consonant structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnalyticConsonantDistance {
    config: AnalyticConsonantDistanceConfig,
}

impl AnalyticConsonantDistance {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: AnalyticConsonantDistanceConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> AnalyticConsonantDistanceConfig {
        self.config
    }

    fn place_distance(&self, c1: &Consonant, c2: &Consonant) -> u32 {
        u32::from(c1.place.abs_diff(c2.place)) * self.config.place_distance_coefficient
    }
}

impl ConsonantDistanceScorer for AnalyticConsonantDistance {
    fn distance(&self, c1: &Consonant, c2: &Consonant) -> u32 {
        if c1.phoneme == c2.phoneme {
            return 0;
        }

        // W/V: special case
        if is_pair(c1, c2, W, V) {
            return self.config.w_v_distance;
        }

        // Both approximants
        if is_approximant(c1) && is_approximant(c2) {
            return if is_pair(c1, c2, R, L) {
                self.config.r_l_distance
            } else {
                self.place_distance(c1, c2)
            };
        }

        let place_diff = self.place_distance(c1, c2);
        let voiced_penalty = if c1.voiced != c2.voiced {
            self.config.voiced_penalty
        } else {
            0
        };
        let sibilance_penalty = if c1.sibilant != c2.sibilant {
            self.config.sibilance_penalty
        } else {
            0
        };

        match (c1.manner, c2.manner) {
            (a, b) if a == b => place_diff + voiced_penalty + sibilance_penalty,
            (Affricate, Fricative | Plosive) | (Fricative | Plosive, Affricate) => {
                let non_affricate = if matches!(c1.manner, Affricate) {
                    c2
                } else {
                    c1
                };
                let penalty = match non_affricate.manner {
                    Fricative if non_affricate.sibilant => {
                        self.config.affricate_sibilant_fricative_penalty
                    }
                    Plosive => self.config.affricate_plosive_penalty,
                    Fricative => self.config.affricate_non_sibilant_fricative_penalty,
                    _ => unreachable!(),
                };
                place_diff + voiced_penalty + penalty
            }
            _ => self.config.unrelated_penalty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phoneme::Consonant;

    fn c(p: crate::phoneme::ConsonantPhoneme) -> Consonant {
        Consonant::from(p)
    }

    fn place_diff(a: crate::phoneme::ConsonantPhoneme, b: crate::phoneme::ConsonantPhoneme) -> u32 {
        c(a).place.abs_diff(c(b).place) as u32
    }

    fn config() -> AnalyticConsonantDistanceConfig {
        AnalyticConsonantDistanceConfig::default()
    }

    fn distance(a: crate::phoneme::ConsonantPhoneme, b: crate::phoneme::ConsonantPhoneme) -> u32 {
        AnalyticConsonantDistance::new().distance(&c(a), &c(b))
    }

    #[test]
    fn consonant_distance_same() {
        assert_eq!(distance(B, B), 0);
    }

    #[test]
    // BOTH PLOSIVE
    fn consonant_distance_same_manner_plosive() {
        assert_eq!(distance(B, K), place_diff(B, K) + config().voiced_penalty);
    }

    #[test]
    // BOTH FRICATIVE
    fn consonant_distance_same_manner_fricative() {
        assert_eq!(distance(F, HH), place_diff(F, HH));
    }

    #[test]
    fn consonant_distance_sibilance_mismatch() {
        assert_eq!(
            distance(S, TH),
            place_diff(S, TH) + config().sibilance_penalty
        );
    }

    #[test]
    fn consonant_distance_voicing_mismatch_is_not_sibilance_mismatch() {
        assert_eq!(distance(DH, TH), config().voiced_penalty);
    }

    #[test]
    // BOTH AFFRICATE
    fn consonant_distance_same_manner_affricate() {
        assert_eq!(
            distance(CH, JH),
            place_diff(CH, JH) + config().voiced_penalty
        );
    }

    #[test]
    // BOTH NASAL
    fn consonant_distance_same_manner_nasal() {
        assert_eq!(distance(M, NG), place_diff(M, NG));
    }

    #[test]
    // R & L
    fn consonant_distance_r_l() {
        assert_eq!(distance(R, L), config().r_l_distance);
    }

    #[test]
    // W & V
    fn consonant_distance_w_v() {
        assert_eq!(distance(W, V), config().w_v_distance);
    }

    #[test]
    // APPROXIMANTS
    fn consonant_distance_approximants() {
        assert_eq!(distance(W, L), place_diff(W, L));
    }

    #[test]
    // ONE AFFRICATE ONE SIBILANT FRICATIVE
    fn consonant_distance_affricate_sibilant_fricative() {
        assert_eq!(
            distance(CH, SH),
            place_diff(CH, SH) + config().affricate_sibilant_fricative_penalty
        );
    }

    #[test]
    // ONE AFFRICATE ONE NON SIBILANT FRICATIVE
    fn consonant_distance_affricate_non_sibilant_fricative() {
        assert_eq!(
            distance(CH, TH),
            place_diff(CH, TH) + config().affricate_non_sibilant_fricative_penalty
        );
    }

    #[test]
    // ONE AFFRICATE ONE PLOSIVE
    fn consonant_distance_affricate_plosive() {
        assert_eq!(
            distance(CH, T),
            place_diff(CH, T) + config().affricate_plosive_penalty
        );
    }

    #[test]
    // UNRELATED CONSONANTS
    fn consonant_distance_unrelated() {
        assert_eq!(distance(M, F), config().unrelated_penalty);
    }

    #[test]
    fn consonant_distance_is_symmetric() {
        assert_eq!(distance(B, K), distance(K, B));
        assert_eq!(distance(CH, SH), distance(SH, CH));
        assert_eq!(distance(W, V), distance(V, W));
    }

    #[test]
    fn custom_config_controls_penalties() {
        let scorer = AnalyticConsonantDistance::with_config(AnalyticConsonantDistanceConfig {
            place_distance_coefficient: 3,
            unrelated_penalty: 30,
            voiced_penalty: 4,
            sibilance_penalty: 10,
            r_l_distance: 5,
            w_v_distance: 6,
            affricate_sibilant_fricative_penalty: 7,
            affricate_plosive_penalty: 8,
            affricate_non_sibilant_fricative_penalty: 9,
        });

        assert_eq!(scorer.distance(&c(M), &c(F)), 30);
        assert_eq!(scorer.distance(&c(B), &c(K)), place_diff(B, K) * 3 + 4);
        assert_eq!(scorer.distance(&c(T), &c(D)), 4);
        assert_eq!(scorer.distance(&c(S), &c(TH)), place_diff(S, TH) * 3 + 10);
        assert_eq!(scorer.distance(&c(R), &c(L)), 5);
        assert_eq!(scorer.distance(&c(W), &c(V)), 6);
        assert_eq!(scorer.distance(&c(CH), &c(SH)), place_diff(CH, SH) * 3 + 7);
        assert_eq!(scorer.distance(&c(CH), &c(T)), place_diff(CH, T) * 3 + 8);
        assert_eq!(scorer.distance(&c(CH), &c(TH)), place_diff(CH, TH) * 3 + 9);
    }
}
