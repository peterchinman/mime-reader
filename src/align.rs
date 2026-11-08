//! The shared phoneme-alignment engine behind rhyme and echo scoring.
//!
//! # One engine, two instruments
//!
//! Every aligner here *maximizes* an `i64` score over alignments of two
//! phoneme sequences. Rhyme distance and echo similarity are both instances:
//!
//! - **Rhyme** aligns two pre-extracted rhyming parts completely and scores
//!   `−cost`: every pair and gap contributes the negation of the rhyme DP's
//!   cost, so the best score is the negated rhyme distance and the winning
//!   alignment is the same one the cost-minimizing DP picks.
//! - **Echo** scores similarity: alike phonemes score positive, unrelated
//!   ones negative, so alignments may leave material unaligned (local and
//!   semi-global modes, masking) without paying for it.
//!
//! For *global* alignment the two conventions are equivalent (Smith,
//! Waterman & Beyer 1981). Every phoneme is consumed exactly once, so give
//! each phoneme any mass `m` and set `sim(p, q) = m(p) + m(q) − cost(p, q)`
//! and `gap(p) = m(p) − cost_gap(p)`: then every alignment satisfies
//! `sim = Σm(A) + Σm(B) − cost`. The same alignment wins and the cost is
//! recoverable exactly. Rhyme is the `m = 0` instance. A positive zero point
//! only changes the answer once phonemes may go unaligned for free — which
//! echo needs and rhyme does not.
//!
//! Everything else that differs between the instruments — the gap model,
//! transposition, weights, normalization — is a parameter. What stays
//! distinct is the question each asks: rhyme chooses by stress what to
//! compare before aligning and keeps strict order; echo lets alignment choose
//! the region and tolerates moved blocks.
//!
//! # Pieces
//!
//! - [`ScoreTable`]: pair scores for every phoneme pair, indexed by
//!   [`phoneme_id`].
//! - [`GapModel`] / [`GapScores`]: a gap of length `L` scores
//!   `−(open + Σ extend)`, with class- and context-aware terms (a consonant
//!   repeating its predecessor is cheaper to gap).
//! - [`TraceEvent`] / [`Trace`]: what a traceback emits, convertible to
//!   [`AlignmentStep`]s.
//! - `linear`: the single-state recurrence with optional adjacent-consonant
//!   swaps (rhyme's DP).
//! - `affine`: the three-state Gotoh recurrence in global, semi-global and
//!   local modes, with masks (echo's aligners).

use crate::phoneme::{Consonant, ConsonantPhoneme, Phoneme, Stress, Vowel, VowelPhoneme};

pub(crate) mod affine;
mod gaps;
pub(crate) mod linear;
#[cfg(test)]
pub(crate) mod oracle;
mod table;
mod trace;

pub use gaps::{GapModel, GapScores};
pub use table::ScoreTable;
pub use trace::{Trace, TraceEvent};

/// The "impossible" score: the identity for `max`, far enough from
/// `i64::MIN` that adding any realistic run of finite scores to it can't
/// overflow. Used as a starting best or a hard wall (masked cells,
/// vowel×consonant pairs in echo's table). At most two are ever summed
/// (`NEG_INF` pair + `NEG_INF` entry) before flooring back to it, which the
/// `/ 4` headroom allows.
pub const NEG_INF: i64 = i64::MIN / 4;

/// How many distinct phonemes there are: 24 consonants, plus 15 vowels × 3
/// stresses.
pub const PHONEME_IDS: usize = CONSONANT_IDS + VowelPhoneme::ALL.len() * STRESSES.len();

/// Phoneme ids below this are consonants; the rest are vowels.
pub(crate) const CONSONANT_IDS: usize = ConsonantPhoneme::ALL.len();

const STRESSES: [Stress; 3] = [Stress::Unstressed, Stress::Primary, Stress::Secondary];

fn stress_index(stress: Stress) -> usize {
    match stress {
        Stress::Unstressed => 0,
        Stress::Primary => 1,
        Stress::Secondary => 2,
    }
}

/// A dense id in `0..PHONEME_IDS` for `phoneme`: consonants first, in
/// declaration order, then each vowel at each stress. Two phonemes have the
/// same id exactly when they are equal.
pub fn phoneme_id(phoneme: &Phoneme) -> u8 {
    let id = match phoneme {
        Phoneme::Consonant(c) => c.phoneme as usize,
        Phoneme::Vowel(v) => {
            CONSONANT_IDS + v.phoneme as usize * STRESSES.len() + stress_index(v.stress)
        }
    };
    id as u8
}

/// The [`phoneme_id`] of every phoneme in `phonemes`, for indexing a
/// [`ScoreTable`].
pub fn phoneme_ids(phonemes: &[Phoneme]) -> Vec<u8> {
    phonemes.iter().map(phoneme_id).collect()
}

/// Whether the phoneme with this id is a consonant.
#[inline]
pub(crate) fn is_consonant_id(id: u8) -> bool {
    usize::from(id) < CONSONANT_IDS
}

/// Every phoneme, indexed by [`phoneme_id`].
pub fn all_phonemes() -> Vec<Phoneme> {
    let consonants = ConsonantPhoneme::ALL
        .iter()
        .map(|&c| Phoneme::Consonant(Consonant::from(c)));
    let vowels = VowelPhoneme::ALL.iter().flat_map(|&phoneme| {
        STRESSES
            .iter()
            .map(move |&stress| Phoneme::Vowel(Vowel { phoneme, stress }))
    });
    consonants.chain(vowels).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspositionDirection {
    /// This phoneme matches a later/right column in the other sequence.
    Right,
    /// This phoneme matches an earlier/left column in the other sequence.
    Left,
}

impl TranspositionDirection {
    pub fn as_char(self) -> char {
        match self {
            TranspositionDirection::Right => '\\',
            TranspositionDirection::Left => '/',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentStep<'a> {
    Match {
        a: &'a Phoneme,
        b: &'a Phoneme,
    },
    Substitution {
        a: &'a Phoneme,
        b: &'a Phoneme,
    },
    Indel {
        a: Option<&'a Phoneme>,
        b: Option<&'a Phoneme>,
    },
    Transposition {
        a: &'a Phoneme,
        b: &'a Phoneme,
        direction: TranspositionDirection,
    },
}

/// An owned [`AlignmentStep`], for keeping an alignment alive beyond the
/// borrowed slices it was computed from (e.g. inside cached analysis results).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedAlignmentStep {
    Match {
        a: Phoneme,
        b: Phoneme,
    },
    Substitution {
        a: Phoneme,
        b: Phoneme,
    },
    Indel {
        a: Option<Phoneme>,
        b: Option<Phoneme>,
    },
    Transposition {
        a: Phoneme,
        b: Phoneme,
        direction: TranspositionDirection,
    },
}

impl OwnedAlignmentStep {
    /// The first-sequence phoneme of this step (`None` for a gap).
    pub fn a(&self) -> Option<&Phoneme> {
        match self {
            OwnedAlignmentStep::Match { a, .. }
            | OwnedAlignmentStep::Substitution { a, .. }
            | OwnedAlignmentStep::Transposition { a, .. } => Some(a),
            OwnedAlignmentStep::Indel { a, .. } => a.as_ref(),
        }
    }

    /// The second-sequence phoneme of this step (`None` for a gap).
    pub fn b(&self) -> Option<&Phoneme> {
        match self {
            OwnedAlignmentStep::Match { b, .. }
            | OwnedAlignmentStep::Substitution { b, .. }
            | OwnedAlignmentStep::Transposition { b, .. } => Some(b),
            OwnedAlignmentStep::Indel { b, .. } => b.as_ref(),
        }
    }
}

impl From<AlignmentStep<'_>> for OwnedAlignmentStep {
    fn from(step: AlignmentStep<'_>) -> Self {
        match step {
            AlignmentStep::Match { a, b } => OwnedAlignmentStep::Match {
                a: a.clone(),
                b: b.clone(),
            },
            AlignmentStep::Substitution { a, b } => OwnedAlignmentStep::Substitution {
                a: a.clone(),
                b: b.clone(),
            },
            AlignmentStep::Indel { a, b } => OwnedAlignmentStep::Indel {
                a: a.cloned(),
                b: b.cloned(),
            },
            AlignmentStep::Transposition { a, b, direction } => OwnedAlignmentStep::Transposition {
                a: a.clone(),
                b: b.clone(),
                direction,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phoneme_ids_are_dense_and_round_trip() {
        let phonemes = all_phonemes();
        assert_eq!(phonemes.len(), PHONEME_IDS);
        assert_eq!(PHONEME_IDS, 69);
        for (id, phoneme) in phonemes.iter().enumerate() {
            assert_eq!(usize::from(phoneme_id(phoneme)), id, "{phoneme}");
            assert_eq!(
                is_consonant_id(id as u8),
                matches!(phoneme, Phoneme::Consonant(_))
            );
        }
    }

    #[test]
    fn parsed_phonemes_get_their_canonical_id() {
        for (s, expected) in [("CH", 0), ("T", 23), ("AE0", 24), ("AE1", 25), ("AE2", 26)] {
            let phoneme: Phoneme = s.parse().unwrap();
            assert_eq!(phoneme_id(&phoneme), expected, "{s}");
        }
        let oy2: Phoneme = "OY2".parse().unwrap();
        assert_eq!(usize::from(phoneme_id(&oy2)), PHONEME_IDS - 1);
    }
}
