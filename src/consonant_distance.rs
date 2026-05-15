use crate::phoneme::{Consonant, ConsonantManner::*, ConsonantPhoneme, ConsonantPhoneme::*};

// furthest related consonants are 7 apart
// unrelated consonants should be... somewhat further than that??
pub const UNRELATED_PENALTY: u32 = 10;

// TIN, DIN
pub const VOICED_PENALTY: u32 = 1;

// ROOT, LOOT
pub const R_L_DISTANCE: u32 = 1;

// WHILE, VIAL
pub const W_V_DISTANCE: u32 = 2;

// CHIN, SHIN
pub const AFFRICATE_SIBILANT_FRICATIVE_PENALTY: u32 = 1;

// CHIN, TIN
pub const AFFRICATE_PLOSIVE_PENALTY: u32 = 2;

// CHIN, THIN
pub const AFFRICATE_NON_SIBILANT_FRICATIVE_PENALTY: u32 = 2;

fn is_pair(a: &Consonant, b: &Consonant, x: ConsonantPhoneme, y: ConsonantPhoneme) -> bool {
    (a.phoneme == x && b.phoneme == y) || (a.phoneme == y && b.phoneme == x)
}

fn is_approximant(c: &Consonant) -> bool {
    matches!(c.manner, Approximant | LateralApproximant)
}

impl Consonant {
    pub fn distance(&self, other: &Consonant) -> u32 {
        if self.phoneme == other.phoneme {
            return 0;
        }

        // W/V: special case
        if is_pair(self, other, W, V) {
            return W_V_DISTANCE;
        }

        // Both approximants
        if is_approximant(self) && is_approximant(other) {
            return if is_pair(self, other, R, L) {
                R_L_DISTANCE
            } else {
                self.place.abs_diff(other.place) as u32
            };
        }

        let place_diff = self.place.abs_diff(other.place) as u32;
        let voiced_penalty = if self.voiced != other.voiced {
            VOICED_PENALTY
        } else {
            0
        };

        match (self.manner, other.manner) {
            (a, b) if a == b => place_diff + voiced_penalty,
            (Affricate, Fricative | Plosive) | (Fricative | Plosive, Affricate) => {
                let non_affricate = if matches!(self.manner, Affricate) {
                    other
                } else {
                    self
                };
                let penalty = match non_affricate.manner {
                    Fricative if non_affricate.sibilant => AFFRICATE_SIBILANT_FRICATIVE_PENALTY,
                    Plosive => AFFRICATE_PLOSIVE_PENALTY,
                    Fricative => AFFRICATE_NON_SIBILANT_FRICATIVE_PENALTY,
                    _ => unreachable!(),
                };
                place_diff + voiced_penalty + penalty
            }
            _ => UNRELATED_PENALTY,
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

    #[test]
    fn consonant_distance_same() {
        assert_eq!(c(B).distance(&c(B)), 0);
    }

    #[test]
    // BOTH PLOSIVE
    fn consonant_distance_same_manner_plosive() {
        assert_eq!(c(B).distance(&c(K)), place_diff(B, K) + VOICED_PENALTY);
    }

    #[test]
    // BOTH FRICATIVE
    fn consonant_distance_same_manner_fricative() {
        assert_eq!(c(F).distance(&c(HH)), place_diff(F, HH));
    }

    #[test]
    // BOTH AFFRICATE
    fn consonant_distance_same_manner_affricate() {
        assert_eq!(c(CH).distance(&c(JH)), place_diff(CH, JH) + VOICED_PENALTY);
    }

    #[test]
    // BOTH NASAL
    fn consonant_distance_same_manner_nasal() {
        assert_eq!(c(M).distance(&c(NG)), place_diff(M, NG));
    }

    #[test]
    // R & L
    fn consonant_distance_r_l() {
        assert_eq!(c(R).distance(&c(L)), R_L_DISTANCE);
    }

    #[test]
    // W & V
    fn consonant_distance_w_v() {
        assert_eq!(c(W).distance(&c(V)), W_V_DISTANCE);
    }

    #[test]
    // APPROXIMANTS
    fn consonant_distance_approximants() {
        assert_eq!(c(W).distance(&c(L)), place_diff(W, L));
    }

    #[test]
    // ONE AFFRICATE ONE SIBILANT FRICATIVE
    fn consonant_distance_affricate_sibilant_fricative() {
        assert_eq!(
            c(CH).distance(&c(SH)),
            place_diff(CH, SH) + AFFRICATE_SIBILANT_FRICATIVE_PENALTY
        );
    }

    #[test]
    // ONE AFFRICATE ONE NON SIBILANT FRICATIVE
    fn consonant_distance_affricate_non_sibilant_fricative() {
        assert_eq!(
            c(CH).distance(&c(TH)),
            place_diff(CH, TH) + AFFRICATE_NON_SIBILANT_FRICATIVE_PENALTY
        );
    }

    #[test]
    // ONE AFFRICATE ONE PLOSIVE
    fn consonant_distance_affricate_plosive() {
        assert_eq!(
            c(CH).distance(&c(T)),
            place_diff(CH, T) + AFFRICATE_PLOSIVE_PENALTY
        );
    }

    #[test]
    // UNRELATED CONSONANTS
    fn consonant_distance_unrelated() {
        assert_eq!(c(M).distance(&c(F)), UNRELATED_PENALTY);
    }

    #[test]
    fn consonant_distance_is_symmetric() {
        assert_eq!(c(B).distance(&c(K)), c(K).distance(&c(B)));
        assert_eq!(c(CH).distance(&c(SH)), c(SH).distance(&c(CH)));
        assert_eq!(c(W).distance(&c(V)), c(V).distance(&c(W)));
    }
}
