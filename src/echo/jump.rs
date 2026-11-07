//! Aligner (5): the jump DP.
//!
//! All of `a` is aligned, left to right, against regions of `b`: after any
//! pair the alignment may resume anywhere in `b` for `move_cost`. That
//! explains moved blocks, permutations and resegmentation in one optimal
//! pass, at the price of allowing `b` material to be reused (charged
//! `reuse_penalty` per extra use, after the fact — the exact no-reuse model
//! is [`super::WatermanEggertAligner`]). Not symmetric: the detector runs
//! both orientations.

use crate::align::{GapScores, ScoreTable, affine};
use crate::phoneme::Phoneme;

use super::alignment::{
    AlignerKind, EchoAligner, EchoCosts, RawAlignment, concatenate, coverage, mark_in_place,
    segments_from_trace,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct JumpAligner;

impl EchoAligner for JumpAligner {
    fn kind(&self) -> AlignerKind {
        AlignerKind::Jump
    }

    fn is_symmetric(&self) -> bool {
        false
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
            affine::Mode::Jump {
                move_cost: costs.move_cost,
            },
            None,
            true,
        );
        let mut segments = segments_from_trace(&trace, a, b, table, &gaps_a, &gaps_b);
        for (segment, _) in segments.iter_mut().skip(1) {
            segment.open_cost = costs.move_cost;
        }
        let (mut segments, steps) = concatenate(segments);
        mark_in_place(&mut segments);
        let (a_coverage, b_coverage) = coverage(&steps, a.len(), b.len());
        let reused: usize = b_coverage
            .iter()
            .map(|&c| usize::from(c.saturating_sub(1)))
            .sum();
        RawAlignment {
            raw: trace.score - costs.reuse_penalty * reused as i64,
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

    const JUMP: JumpAligner = JumpAligner;

    fn moved(alignment: &RawAlignment) -> usize {
        alignment
            .segments
            .iter()
            .filter(|s| s.placement == Placement::Moved)
            .count()
    }

    #[test]
    fn verbatim_never_jumps() {
        let alignment = raw(&JUMP, SWAP_A, SWAP_A);
        assert_eq!(alignment.raw, self_sim(SWAP_A));
        assert_eq!(alignment.segments.len(), 1);
    }

    #[test]
    fn an_insertion_is_one_gap() {
        // Consuming the shorter line, the insertion is one gap in it.
        assert_eq!(
            raw(&JUMP, BIRD, BIRD_INSERTED).raw,
            self_sim(BIRD) - costs().gaps.open
        );
    }

    #[test]
    fn a_two_block_swap_is_one_jump() {
        let alignment = raw(&JUMP, SWAP_A, SWAP_B);
        assert_eq!(alignment.segments.len(), 2);
        assert_eq!(moved(&alignment), 1);
        assert_eq!(alignment.segments[1].open_cost, costs().move_cost);
        let dark_demands_i =
            self_sim("D AA1 R K D IH0 M AE1 N D Z AY1") - sim("Z", "Z") + sim("Z", "S");
        let call_my_own = self_sim("K AO1 L M AY1 OW1 N");
        assert_eq!(
            alignment.raw,
            dark_demands_i + call_my_own - costs().move_cost
        );
    }

    #[test]
    fn block_permutations_count_their_jumps() {
        let a = [BLOCK_X, BLOCK_Y, BLOCK_Z].join(" ");
        let zxy = raw(&JUMP, &a, &[BLOCK_Z, BLOCK_X, BLOCK_Y].join(" "));
        assert_eq!((zxy.segments.len(), moved(&zxy)), (2, 1));
        assert_eq!(zxy.raw, self_sim(&a) - costs().move_cost);
        let zyx = raw(&JUMP, &a, &[BLOCK_Z, BLOCK_Y, BLOCK_X].join(" "));
        assert_eq!((zyx.segments.len(), moved(&zyx)), (3, 2));
        assert_eq!(zyx.raw, self_sim(&a) - 2 * costs().move_cost);
    }

    #[test]
    fn reuse_is_counted_and_charged() {
        let alignment = raw(&JUMP, "L AA1 L AA1 L AA1", "L AA1");
        assert_eq!(alignment.b_coverage, [3, 3]);
        assert_eq!(alignment.reused_b(), 4);
        let la = self_sim("L AA1");
        assert_eq!(alignment.raw, 3 * la - 2 * costs().move_cost);

        let mut config = crate::echo::EchoConfig::default();
        config.structure.reuse_penalty = 100;
        let detector = crate::echo::EchoDetector::with_config(config);
        let charged = JUMP.align(
            &parse("L AA1 L AA1 L AA1"),
            &parse("L AA1"),
            detector.table(),
            detector.costs(),
        );
        assert_eq!(
            charged.raw,
            3 * la - 2 * detector.costs().move_cost - 4 * detector.costs().reuse_penalty
        );
    }

    #[test]
    fn unrelated_lines_score_low() {
        let alignment = raw(&JUMP, UNRELATED_A, UNRELATED_B);
        let similarity = alignment.raw as f64 / self_sim(UNRELATED_A) as f64;
        assert!(similarity < 0.35, "{similarity}");
    }

    #[test]
    fn transpose_reports_the_other_orientation() {
        let forward = raw(&JUMP, "L AA1 L AA1 L AA1", "L AA1");
        let swapped = forward.clone().transpose();
        assert_eq!(swapped.raw, forward.raw);
        assert_eq!(swapped.a_coverage, forward.b_coverage);
        assert_eq!(swapped.reused_a(), 4);
        for step in &swapped.steps {
            assert_eq!(step.step.a().is_some(), step.a_index.is_some());
        }
        let starts: Vec<usize> = swapped.segments.iter().map(|s| s.a_range.start).collect();
        assert!(
            starts.is_sorted(),
            "segments re-sorted into the new a order"
        );
    }
}
