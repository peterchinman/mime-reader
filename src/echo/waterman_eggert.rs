//! Repeated local alignment with masking (Waterman–Eggert).
//!
//! Finds the best shared local alignment, splits it at its gaps into
//! ungapped runs, walls off the phonemes those runs pair in both lines, and
//! repeats until no further alignment scores at least `min_run` (the best
//! one always counts, so a verbatim one-syllable line still echoes itself).
//! The runs are then assembled: the heaviest chain that keeps its order in
//! both lines is in place, the rest are moved blocks. Every phoneme is used
//! at most once.
//!
//! Why split at gaps: a local alignment may bridge two in-order blocks with
//! one cheap gap ("Y ⟨gap⟩ Z" when `a = X Y Z`, `b = Y X Z`). Walling off
//! that alignment's full ranges would also wall off the block hiding in the
//! gap (`X` in `b`), so it could never be recovered as a moved run. Only
//! paired positions are walled; the gap becomes a run boundary, which
//! assembly charges `run_open` — by default the same as `gap_open`.
//!
//! Assembly charges `run_open` per run after the first and `move_cost` per
//! moved run, and keeps a run only if it pays for itself: a run outside the
//! in-place chain is kept, as moved, only if `score > run_open + move_cost`.
//! So `raw = Σ score − run_open × (runs − 1) − move_cost × moved` over the
//! kept runs, and the in-place chain is the one that maximizes it: each run
//! is worth `score − run_open` in place and `max(0, score − run_open −
//! move_cost)` outside, so the chain maximizes `Σ min(score − run_open,
//! move_cost)`.

use crate::align::{GapScores, ScoreTable, affine, phoneme_ids};
use crate::phoneme::Phoneme;

use super::alignment::{
    EchoAligner, EchoCosts, EchoSegment, EchoStep, Placement, RawAlignment, aligned, concatenate,
    mark_in_place, segment_of, segments_from_trace,
};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WatermanEggertAligner;

impl WatermanEggertAligner {
    /// The ungapped runs of every accepted local alignment, in discovery
    /// order.
    fn runs(
        a: &[Phoneme],
        b: &[Phoneme],
        table: &ScoreTable,
        costs: &EchoCosts,
    ) -> Vec<(EchoSegment, Vec<EchoStep>)> {
        let (gaps_a, gaps_b) = (
            GapScores::new(&costs.gaps, a, None),
            GapScores::new(&costs.gaps, b, None),
        );
        let (ids_a, ids_b) = (phoneme_ids(a), phoneme_ids(b));
        let mut mask = affine::Mask::new(a.len(), b.len());
        let mut runs = Vec::new();
        for _ in 0..costs.max_runs {
            let trace = affine::align(
                &ids_a,
                &ids_b,
                table,
                &gaps_a,
                &gaps_b,
                affine::Mode::Local,
                Some(&mask),
                true,
            );
            // The best run always counts; `min_run` keeps noise out of the
            // rest.
            let threshold = if runs.is_empty() {
                1
            } else {
                costs.min_run.max(1)
            };
            if trace.events.is_empty() || trace.score < threshold {
                break;
            }
            let found = segments_from_trace(&trace, a, b, table, &gaps_a, &gaps_b);
            debug_assert_eq!(found.len(), 1, "a local alignment is one segment");
            for (_, steps) in found {
                for run in split_at_gaps(steps) {
                    mask.block(run.0.a_range.clone(), run.0.b_range.clone());
                    runs.push(run);
                }
            }
        }
        runs
    }
}

/// The maximal gap-free stretches of `steps`, each as its own segment.
fn split_at_gaps(steps: Vec<EchoStep>) -> Vec<(EchoSegment, Vec<EchoStep>)> {
    let mut runs = Vec::new();
    let mut current: Vec<EchoStep> = Vec::new();
    for step in steps {
        if step.a_index.is_some() && step.b_index.is_some() {
            current.push(step);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs.into_iter()
        .map(|steps| (segment_of(&steps), steps))
        .collect()
}

/// Places `runs` (sorted by their start in `a`) and keeps those that pay
/// for themselves, with their open costs set (see the module docs).
fn assemble(
    runs: Vec<(EchoSegment, Vec<EchoStep>)>,
    costs: &EchoCosts,
) -> Vec<(EchoSegment, Vec<EchoStep>)> {
    let (mut segments, steps): (Vec<EchoSegment>, Vec<Vec<EchoStep>>) = runs.into_iter().unzip();
    // Every run is weighed as if charged run_open; the first kept run gets
    // it back below, which shifts every chain's total alike.
    let weights: Vec<i64> = segments
        .iter()
        .map(|s| (s.score - costs.run_open).min(costs.move_cost))
        .collect();
    mark_in_place(&mut segments, &weights);
    let mut kept: Vec<(EchoSegment, Vec<EchoStep>)> = segments
        .into_iter()
        .zip(steps)
        .filter(|(segment, _)| {
            segment.placement == Placement::InPlace
                || segment.score > costs.run_open + costs.move_cost
        })
        .collect();
    for (index, (segment, _)) in kept.iter_mut().enumerate() {
        let run_open = if index == 0 { 0 } else { costs.run_open };
        let moved = if segment.placement == Placement::Moved {
            costs.move_cost
        } else {
            0
        };
        segment.open_cost = run_open + moved;
    }
    kept
}

impl EchoAligner for WatermanEggertAligner {
    /// Greedy: when several runs tie, which is taken first (and so walled
    /// off) depends on orientation, and can change what is left.
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
        let mut runs = Self::runs(a, b, table, costs);
        if runs.is_empty() {
            return RawAlignment::empty(a.len(), b.len());
        }
        runs.sort_by_key(|(segment, _)| segment.a_range.start);
        let (segments, steps) = concatenate(assemble(runs, costs));
        let raw = segments.iter().map(|s| s.score - s.open_cost).sum();
        let (a_aligned, b_aligned) = aligned(&steps, a.len(), b.len());
        RawAlignment {
            raw,
            steps,
            segments,
            a_aligned,
            b_aligned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo::Placement;
    use crate::echo::test_support::*;

    const WE: WatermanEggertAligner = WatermanEggertAligner;

    /// A run of `score` over `a` and `b`, with no steps (assembly only
    /// reads ranges and scores).
    fn run(
        a: std::ops::Range<usize>,
        b: std::ops::Range<usize>,
        score: i64,
    ) -> (EchoSegment, Vec<EchoStep>) {
        let segment = EchoSegment {
            a_range: a,
            b_range: b,
            steps: 0..0,
            score,
            open_cost: 0,
            placement: Placement::InPlace,
        };
        (segment, Vec::new())
    }

    fn assembled_raw(kept: &[(EchoSegment, Vec<EchoStep>)]) -> i64 {
        kept.iter().map(|(s, _)| s.score - s.open_cost).sum()
    }

    #[test]
    fn assembly_moves_a_heavy_run_when_that_keeps_more() {
        // B (10 units) comes after P, Q, R (2 each) in `a` but before them
        // in `b`. Keeping B in place would move all three (raw 13); moving
        // B instead keeps them in place (raw 14).
        let unit = 1000;
        let costs = EchoCosts {
            run_open: unit / 2,
            move_cost: unit / 2,
            ..*costs()
        };
        let runs = vec![
            run(0..2, 10..12, 2 * unit),
            run(2..4, 12..14, 2 * unit),
            run(4..6, 14..16, 2 * unit),
            run(6..16, 0..10, 10 * unit),
        ];
        let kept = assemble(runs, &costs);
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[3].0.placement, Placement::Moved);
        // 10 + 3 × 2 − 3 run opens − 1 move.
        assert_eq!(assembled_raw(&kept), 14 * unit);
    }

    #[test]
    fn assembly_keeps_a_light_run_in_place_rather_than_moving_a_heavy_one() {
        // A light run conflicting with B: B stays in place, and the light
        // run, not worth a move, is dropped.
        let unit = 1000;
        let costs = EchoCosts {
            run_open: unit / 2,
            move_cost: unit / 2,
            ..*costs()
        };
        let runs = vec![
            run(0..2, 10..12, unit / 2 + 100),
            run(6..16, 0..10, 10 * unit),
        ];
        let kept = assemble(runs, &costs);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0.placement, Placement::InPlace);
        assert_eq!(assembled_raw(&kept), 10 * unit);
    }

    #[test]
    fn verbatim_is_one_in_place_run() {
        let alignment = raw(&WE, SWAP_A, SWAP_A);
        assert_eq!(alignment.raw, self_sim(SWAP_A));
        assert_eq!(alignment.segments.len(), 1);
        assert_eq!(alignment.segments[0].placement, Placement::InPlace);
    }

    #[test]
    fn an_insertion_costs_one_run_open() {
        let alignment = raw(&WE, BIRD, BIRD_INSERTED);
        assert_eq!(alignment.raw, self_sim(BIRD) - costs().run_open);
        assert_eq!(
            costs().run_open,
            costs().gaps.open,
            "defaults: a run boundary costs a gap"
        );
        assert_eq!(alignment.segments.len(), 2);
        assert!(
            alignment
                .segments
                .iter()
                .all(|s| s.placement == Placement::InPlace)
        );
    }

    #[test]
    fn a_two_block_swap_is_one_moved_run() {
        let alignment = raw(&WE, SWAP_A, SWAP_B);
        let moved: Vec<_> = alignment
            .segments
            .iter()
            .filter(|s| s.placement == Placement::Moved)
            .collect();
        assert_eq!(moved.len(), 1);
        // "dark demands I" in place (with Z→S), "call my own" moved.
        let dark_demands_i =
            self_sim("D AA1 R K D IH0 M AE1 N D Z AY1") - sim("Z", "Z") + sim("Z", "S");
        let call_my_own = self_sim("K AO1 L M AY1 OW1 N");
        assert_eq!(
            alignment.raw,
            dark_demands_i + call_my_own - costs().run_open - costs().move_cost
        );
    }

    #[test]
    fn block_permutations_count_their_moves() {
        let a = [BLOCK_X, BLOCK_Y, BLOCK_Z].join(" ");
        let total = self_sim(&a);
        let zxy = raw(&WE, &a, &[BLOCK_Z, BLOCK_X, BLOCK_Y].join(" "));
        assert_eq!(
            zxy.segments
                .iter()
                .filter(|s| s.placement == Placement::Moved)
                .count(),
            1
        );
        assert_eq!(zxy.raw, total - costs().run_open - costs().move_cost);
        let zyx = raw(&WE, &a, &[BLOCK_Z, BLOCK_Y, BLOCK_X].join(" "));
        assert_eq!(
            zyx.segments
                .iter()
                .filter(|s| s.placement == Placement::Moved)
                .count(),
            2
        );
        assert_eq!(
            zyx.raw,
            total - 2 * costs().run_open - 2 * costs().move_cost
        );
    }

    #[test]
    fn a_block_hidden_in_a_runs_gap_is_still_found() {
        // The best local alignment is "Y ⟨gap over b's X⟩ Z"; X must still
        // be recovered as a moved run.
        let a = [BLOCK_X, BLOCK_Y, BLOCK_Z].join(" ");
        let yxz = raw(&WE, &a, &[BLOCK_Y, BLOCK_X, BLOCK_Z].join(" "));
        assert!(
            yxz.a_aligned.iter().all(|&x| x),
            "every phoneme aligned once"
        );
        assert_eq!(
            yxz.segments
                .iter()
                .filter(|s| s.placement == Placement::Moved)
                .count(),
            1
        );
        assert_eq!(
            yxz.raw,
            self_sim(&a) - 2 * costs().run_open - costs().move_cost
        );
    }

    #[test]
    fn runs_that_do_not_pay_are_dropped() {
        // A lone shared primary vowel out of order is a run (≥ min_run) but
        // can't pay run_open + move_cost.
        let alignment = raw(&WE, "K AE1 T S IH1 T", "S IH1 T M AE1 P");
        assert!(
            alignment
                .segments
                .iter()
                .all(|s| s.score > costs().run_open + costs().move_cost
                    || s.placement == Placement::InPlace)
        );
    }

    #[test]
    fn unrelated_lines_share_a_little() {
        let alignment = raw(&WE, UNRELATED_A, UNRELATED_B);
        assert!(alignment.raw > 0);
        let similarity =
            alignment.raw as f64 / self_sim(UNRELATED_A).min(self_sim(UNRELATED_B)) as f64;
        assert!(similarity < 0.35, "{similarity}");
    }

    #[test]
    fn no_phoneme_is_used_twice() {
        let alphabet = parse("AE1 AE0 IH0 OW1 ER0 K T T S Z M N D L");
        let mut rng = Rng(0x5DEE_CE66_D1CE_4E5B);
        for _ in 0..500 {
            let (a, b) = (rng.sequence(&alphabet, 16), rng.sequence(&alphabet, 16));
            let alignment = WE.align(&a, &b, detector().table(), costs());
            let (mut a_uses, mut b_uses) = (vec![0; a.len()], vec![0; b.len()]);
            for step in &alignment.steps {
                if let (Some(i), Some(j)) = (step.a_index, step.b_index) {
                    a_uses[i] += 1;
                    b_uses[j] += 1;
                }
            }
            assert!(a_uses.iter().chain(&b_uses).all(|&uses| uses <= 1));
            // Kept runs are disjoint in b as well as a.
            for (x, s) in alignment.segments.iter().enumerate() {
                for t in &alignment.segments[x + 1..] {
                    assert!(s.a_range.end <= t.a_range.start, "sorted, disjoint in a");
                    assert!(s.b_range.end <= t.b_range.start || t.b_range.end <= s.b_range.start);
                }
            }
        }
    }
}
