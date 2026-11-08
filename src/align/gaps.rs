//! The gap model: what leaving phonemes unaligned costs.

use crate::phoneme::{Phoneme, is_repeated_consonant};

/// Costs of a gap — a run of consecutive phonemes in one sequence aligned to
/// nothing in the other. A gap of length `L` over phonemes `p₁..p_L` costs
/// `open(p₁) + Σ extend(pᵢ)`, i.e. scores `−(open + Σ extend)`.
///
/// Both terms are class- and context-aware: a consonant that repeats its
/// predecessor *in the same sequence* (the second T of "TEN TO" against
/// "TENT O") is a repeated consonant and takes the `*_repeated_consonant`
/// variant. The predecessor is the previous phoneme of the sequence itself,
/// whether or not it was gapped too.
///
/// Rhyme uses `open = 0` and its per-phoneme indel penalties as `extend`, so
/// a gap is the sum of its phonemes' penalties. Echo charges one `open` per
/// gap and (by default) nothing per phoneme, so an inserted phrase costs one
/// event however long it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GapModel {
    /// Opening a gap whose first phoneme is not a repeated consonant.
    pub open: i64,
    /// Opening a gap whose first phoneme is a repeated consonant.
    pub open_repeated_consonant: i64,
    /// Each gapped vowel.
    pub extend_vowel: i64,
    /// Each gapped consonant that is not a repeated consonant.
    pub extend_consonant: i64,
    /// Each gapped repeated consonant.
    pub extend_repeated_consonant: i64,
}

impl GapModel {
    /// The cost of opening a gap at `phoneme`, given its predecessor in the
    /// same sequence.
    pub fn open_cost(&self, phoneme: &Phoneme, prev: Option<&Phoneme>) -> i64 {
        if is_repeated_consonant(phoneme, prev) {
            self.open_repeated_consonant
        } else {
            self.open
        }
    }

    /// The cost of gapping `phoneme` (beyond any opening cost), given its
    /// predecessor in the same sequence.
    pub fn extend_cost(&self, phoneme: &Phoneme, prev: Option<&Phoneme>) -> i64 {
        match phoneme {
            Phoneme::Vowel(_) => self.extend_vowel,
            Phoneme::Consonant(_) if is_repeated_consonant(phoneme, prev) => {
                self.extend_repeated_consonant
            }
            Phoneme::Consonant(_) => self.extend_consonant,
        }
    }
}

/// A [`GapModel`] evaluated at every position of one sequence, so the DP
/// reads gap costs by index instead of re-deriving the context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapScores {
    /// `open[i]`: the cost of opening a gap at phoneme `i`.
    pub open: Vec<i64>,
    /// `extend[i]`: the cost of gapping phoneme `i`.
    pub extend: Vec<i64>,
}

impl GapScores {
    /// Evaluates `model` along `phonemes`. `prefix_context` stands in as the
    /// predecessor of phoneme 0 — e.g. the last phoneme of a common prefix
    /// stripped before aligning, so the repeated-consonant discount survives
    /// the strip.
    pub fn new(model: &GapModel, phonemes: &[Phoneme], prefix_context: Option<&Phoneme>) -> Self {
        let mut open = Vec::with_capacity(phonemes.len());
        let mut extend = Vec::with_capacity(phonemes.len());
        let mut prev = prefix_context;
        for phoneme in phonemes {
            open.push(model.open_cost(phoneme, prev));
            extend.push(model.extend_cost(phoneme, prev));
            prev = Some(phoneme);
        }
        Self { open, extend }
    }

    pub fn len(&self) -> usize {
        self.extend.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    const MODEL: GapModel = GapModel {
        open: 50,
        open_repeated_consonant: 20,
        extend_vowel: 7,
        extend_consonant: 5,
        extend_repeated_consonant: 2,
    };

    #[test]
    fn costs_follow_class_and_same_sequence_context() {
        let s = parse("T T AE1 T D");
        let gaps = GapScores::new(&MODEL, &s, None);
        assert_eq!(gaps.extend, [5, 2, 7, 5, 5]);
        assert_eq!(gaps.open, [50, 20, 50, 50, 50]);
    }

    #[test]
    fn prefix_context_stands_in_before_the_first_phoneme() {
        let t: Phoneme = "T".parse().unwrap();
        let s = parse("T AE1");
        assert_eq!(GapScores::new(&MODEL, &s, Some(&t)).extend, [2, 7]);
        assert_eq!(GapScores::new(&MODEL, &s, Some(&t)).open, [20, 50]);
        assert_eq!(GapScores::new(&MODEL, &s, None).extend, [5, 7]);
    }

    #[test]
    fn a_vowel_is_never_a_repeated_consonant() {
        let s = parse("AE1 AE1");
        assert_eq!(GapScores::new(&MODEL, &s, None).extend, [7, 7]);
        assert_eq!(GapScores::new(&MODEL, &s, None).open, [50, 50]);
    }
}
