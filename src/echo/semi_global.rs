//! Aligner (1): one semi-global alignment.
//!
//! Free end gaps let one line be a truncation or extension of the other at
//! either end; inside, everything stays in order and an inserted phrase is
//! one gap. Re-ordered blocks can only be explained as gaps and
//! substitutions, so this is the strict baseline.

use crate::align::{GapScores, ScoreTable, affine};
use crate::phoneme::Phoneme;

use super::alignment::{
    AlignerKind, EchoAligner, EchoCosts, RawAlignment, concatenate, coverage, segments_from_trace,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct SemiGlobalAligner;

impl EchoAligner for SemiGlobalAligner {
    fn kind(&self) -> AlignerKind {
        AlignerKind::SemiGlobal
    }

    fn align(
        &self,
        a: &[Phoneme],
        b: &[Phoneme],
        table: &ScoreTable,
        costs: &EchoCosts,
    ) -> RawAlignment {
        let (gaps_a, gaps_b) = (
            GapScores::new(&costs.gaps, a, None),
            GapScores::new(&costs.gaps, b, None),
        );
        let trace = affine::align(
            &ScoreTable::ids(a),
            &ScoreTable::ids(b),
            table,
            &gaps_a,
            &gaps_b,
            affine::Mode::SemiGlobal,
            None,
            true,
        );
        let (segments, steps) =
            concatenate(segments_from_trace(&trace, a, b, table, &gaps_a, &gaps_b));
        let (a_coverage, b_coverage) = coverage(&steps, a.len(), b.len());
        RawAlignment {
            raw: trace.score,
            steps,
            segments,
            a_coverage,
            b_coverage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo::Placement;
    use crate::echo::test_support::*;

    const SG: SemiGlobalAligner = SemiGlobalAligner;

    #[test]
    fn verbatim_is_one_in_place_segment() {
        let alignment = raw(&SG, SWAP_A, SWAP_A);
        assert_eq!(alignment.raw, self_sim(SWAP_A));
        assert_eq!(alignment.segments.len(), 1);
        assert_eq!(alignment.segments[0].placement, Placement::InPlace);
        assert!(alignment.a_coverage.iter().all(|&c| c == 1));
    }

    #[test]
    fn a_substitution_costs_exactly_its_pair() {
        // "demands I" vs "demand sigh": boundary-blind, just Z→S.
        let (a, b) = ("D IH0 M AE1 N D Z AY1", "D IH0 M AE1 N D S AY1");
        assert_eq!(
            raw(&SG, a, b).raw,
            self_sim(a) - sim("Z", "Z") + sim("Z", "S")
        );
    }

    #[test]
    fn an_insertion_is_one_gap() {
        assert_eq!(
            raw(&SG, BIRD, BIRD_INSERTED).raw,
            self_sim(BIRD) - costs().gaps.open
        );
    }

    #[test]
    fn truncation_is_free() {
        assert_eq!(
            raw(&SG, "D AA1 R K", "D AA1 R K ER0").raw,
            self_sim("D AA1 R K")
        );
        assert_eq!(
            raw(&SG, "DH AH0 D AA1 R K", "D AA1 R K").raw,
            self_sim("D AA1 R K")
        );
    }

    #[test]
    fn re_ordering_is_not_recognized() {
        let alignment = raw(&SG, SWAP_A, SWAP_B);
        let similarity = alignment.raw as f64 / self_sim(SWAP_A) as f64;
        assert!(similarity < 0.7, "{similarity}");
    }

    #[test]
    fn unrelated_lines_score_low() {
        let alignment = raw(&SG, UNRELATED_A, UNRELATED_B);
        let similarity =
            alignment.raw as f64 / self_sim(UNRELATED_A).min(self_sim(UNRELATED_B)) as f64;
        assert!(similarity < 0.35, "{similarity}");
    }
}
