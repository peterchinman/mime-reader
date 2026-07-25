use crate::NORMALIZED_DISTANCE_MAX;
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
    /// Distance on the shared normalized scale: `0` (identical) through
    /// [`NORMALIZED_DISTANCE_MAX`] (the most distant consonant pair this
    /// engine can produce).
    fn distance(&self, c1: &Consonant, c2: &Consonant) -> u32;
}

/// Implements a consonant distance score derived from my own analysis of consonant structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalyticConsonantDistance {
    config: AnalyticConsonantDistanceConfig,
    /// The largest raw (pre-normalization) distance any consonant pair
    /// produces under this config. Denominator for normalization; found by
    /// enumerating all pairs so it can never drift from `raw_distance`.
    max_raw: u32,
}

impl AnalyticConsonantDistance {
    pub fn new() -> Self {
        Self::with_config(AnalyticConsonantDistanceConfig::default())
    }

    pub fn with_config(config: AnalyticConsonantDistanceConfig) -> Self {
        let mut scorer = Self { config, max_raw: 0 };
        scorer.max_raw = ConsonantPhoneme::ALL
            .iter()
            .flat_map(|&a| {
                ConsonantPhoneme::ALL
                    .iter()
                    .map(move |&b| scorer.raw_distance(&Consonant::from(a), &Consonant::from(b)))
            })
            .max()
            .unwrap_or(0);
        scorer
    }

    pub fn config(&self) -> AnalyticConsonantDistanceConfig {
        self.config
    }

    fn place_distance(&self, c1: &Consonant, c2: &Consonant) -> u32 {
        u32::from(c1.place.abs_diff(c2.place)) * self.config.place_distance_coefficient
    }

    /// Scales a raw distance onto `0..=NORMALIZED_DISTANCE_MAX`
    /// (round-to-nearest). Raw distances are at most `max_raw` by
    /// construction, so the result never exceeds the scale.
    fn normalize(&self, raw: u32) -> u32 {
        if self.max_raw == 0 {
            return 0;
        }
        ((u64::from(raw) * u64::from(NORMALIZED_DISTANCE_MAX) + u64::from(self.max_raw / 2))
            / u64::from(self.max_raw)) as u32
    }

    /// The unnormalized distance, in this engine's own units.
    fn raw_distance(&self, c1: &Consonant, c2: &Consonant) -> u32 {
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

impl ConsonantDistanceScorer for AnalyticConsonantDistance {
    fn distance(&self, c1: &Consonant, c2: &Consonant) -> u32 {
        self.normalize(self.raw_distance(c1, c2))
    }
}

impl Default for AnalyticConsonantDistance {
    // Not derived: `max_raw` must be computed from the config, and a derived
    // default would zero it.
    fn default() -> Self {
        Self::new()
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

    /// The expected normalized distance for a raw (pre-scaling) distance
    /// under the default config.
    fn norm(raw: u32) -> u32 {
        AnalyticConsonantDistance::new().normalize(raw)
    }

    #[test]
    fn consonant_distance_same() {
        assert_eq!(distance(B, B), 0);
    }

    #[test]
    // BOTH PLOSIVE
    fn consonant_distance_same_manner_plosive() {
        assert_eq!(
            distance(B, K),
            norm(place_diff(B, K) + config().voiced_penalty)
        );
    }

    #[test]
    // BOTH FRICATIVE
    fn consonant_distance_same_manner_fricative() {
        assert_eq!(distance(F, HH), norm(place_diff(F, HH)));
    }

    #[test]
    fn consonant_distance_sibilance_mismatch() {
        assert_eq!(
            distance(S, TH),
            norm(place_diff(S, TH) + config().sibilance_penalty)
        );
    }

    #[test]
    fn consonant_distance_voicing_mismatch_is_not_sibilance_mismatch() {
        assert_eq!(distance(DH, TH), norm(config().voiced_penalty));
    }

    #[test]
    // BOTH AFFRICATE
    fn consonant_distance_same_manner_affricate() {
        assert_eq!(
            distance(CH, JH),
            norm(place_diff(CH, JH) + config().voiced_penalty)
        );
    }

    #[test]
    // BOTH NASAL
    fn consonant_distance_same_manner_nasal() {
        assert_eq!(distance(M, NG), norm(place_diff(M, NG)));
    }

    #[test]
    // R & L
    fn consonant_distance_r_l() {
        assert_eq!(distance(R, L), norm(config().r_l_distance));
    }

    #[test]
    // W & V
    fn consonant_distance_w_v() {
        assert_eq!(distance(W, V), norm(config().w_v_distance));
    }

    #[test]
    // APPROXIMANTS
    fn consonant_distance_approximants() {
        assert_eq!(distance(W, L), norm(place_diff(W, L)));
    }

    #[test]
    // ONE AFFRICATE ONE SIBILANT FRICATIVE
    fn consonant_distance_affricate_sibilant_fricative() {
        assert_eq!(
            distance(CH, SH),
            norm(place_diff(CH, SH) + config().affricate_sibilant_fricative_penalty)
        );
    }

    #[test]
    // ONE AFFRICATE ONE NON SIBILANT FRICATIVE
    fn consonant_distance_affricate_non_sibilant_fricative() {
        assert_eq!(
            distance(CH, TH),
            norm(place_diff(CH, TH) + config().affricate_non_sibilant_fricative_penalty)
        );
    }

    #[test]
    // ONE AFFRICATE ONE PLOSIVE
    fn consonant_distance_affricate_plosive() {
        assert_eq!(
            distance(CH, T),
            norm(place_diff(CH, T) + config().affricate_plosive_penalty)
        );
    }

    #[test]
    // UNRELATED CONSONANTS
    fn consonant_distance_unrelated() {
        assert_eq!(distance(M, F), norm(config().unrelated_penalty));
        // Under the default config no related pair exceeds the unrelated
        // penalty, so unrelated pairs sit exactly at the top of the scale.
        assert_eq!(distance(M, F), NORMALIZED_DISTANCE_MAX);
    }

    /// The normalized-scale contract: every pair stays within
    /// `0..=NORMALIZED_DISTANCE_MAX` and the most distant pair lands exactly
    /// on the top of the scale.
    #[test]
    fn distances_span_the_normalized_scale() {
        let mut max = 0;
        for a in ConsonantPhoneme::ALL {
            for b in ConsonantPhoneme::ALL {
                let d = distance(a, b);
                assert!(d <= NORMALIZED_DISTANCE_MAX, "{a} vs {b}: {d}");
                max = max.max(d);
            }
        }
        assert_eq!(max, NORMALIZED_DISTANCE_MAX);
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

        assert_eq!(scorer.distance(&c(M), &c(F)), scorer.normalize(30));
        assert_eq!(
            scorer.distance(&c(B), &c(K)),
            scorer.normalize(place_diff(B, K) * 3 + 4)
        );
        assert_eq!(scorer.distance(&c(T), &c(D)), scorer.normalize(4));
        assert_eq!(
            scorer.distance(&c(S), &c(TH)),
            scorer.normalize(place_diff(S, TH) * 3 + 10)
        );
        assert_eq!(scorer.distance(&c(R), &c(L)), scorer.normalize(5));
        assert_eq!(scorer.distance(&c(W), &c(V)), scorer.normalize(6));
        assert_eq!(
            scorer.distance(&c(CH), &c(SH)),
            scorer.normalize(place_diff(CH, SH) * 3 + 7)
        );
        assert_eq!(
            scorer.distance(&c(CH), &c(T)),
            scorer.normalize(place_diff(CH, T) * 3 + 8)
        );
        assert_eq!(
            scorer.distance(&c(CH), &c(TH)),
            scorer.normalize(place_diff(CH, TH) * 3 + 9)
        );
    }
}
