//! The aligner interface and what an echo alignment reports.

use std::ops::Range;

use crate::align::{GapModel, GapScores, OwnedAlignmentStep, ScoreTable, Trace, TraceEvent};
use crate::phoneme::Phoneme;

use super::EchoNormalization;

/// Which alignment strategy scores an echo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AlignerKind {
    /// (1) One semi-global alignment: free end gaps, strict order.
    SemiGlobal,
    /// (3) Repeated local alignment with masking: disjoint shared runs,
    /// assembled in or out of order.
    #[default]
    WatermanEggert,
    /// (5) Jump DP: all of one line against regions of the other, resuming
    /// anywhere (reuse allowed) for a move cost.
    Jump,
}

impl AlignerKind {
    pub const ALL: [Self; 3] = [Self::SemiGlobal, Self::WatermanEggert, Self::Jump];

    /// The snake_case name used on the command line and in JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::SemiGlobal => "semi_global",
            Self::WatermanEggert => "waterman_eggert",
            Self::Jump => "jump",
        }
    }
}

impl std::fmt::Display for AlignerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for AlignerKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == s)
            .ok_or_else(|| {
                format!("unknown aligner {s:?} (expected semi_global, waterman_eggert or jump)")
            })
    }
}

/// Where a segment of `a` landed in `b` relative to the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Part of the heaviest chain of segments that keeps its order in both
    /// lines.
    InPlace,
    /// Out of order relative to that chain: a moved block.
    Moved,
}

/// Echo costs in engine units (see [`super::EchoDetector::unit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EchoCosts {
    pub gaps: GapModel,
    /// Charged per moved segment (aligner 3) or per jump (aligner 5).
    pub move_cost: i64,
    /// Charged per local run after the first (aligner 3).
    pub run_open: i64,
    /// Local runs scoring below this end the search (aligner 3).
    pub min_run: i64,
    /// Charged per extra use of a `b` phoneme (aligner 5).
    pub reuse_penalty: i64,
    /// At most this many local runs (aligner 3).
    pub max_runs: usize,
}

/// One aligned step with its positions and its contribution to the score:
/// a pair's similarity, or a gap's `−(open + extend)` (open only on the
/// gap's first phoneme).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EchoStep {
    pub step: OwnedAlignmentStep,
    pub a_index: Option<usize>,
    pub b_index: Option<usize>,
    pub score: i64,
}

/// A contiguous aligned stretch: a semi-global alignment, one local run, or
/// the stretch between two jumps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EchoSegment {
    /// The positions of `a` the segment's steps consume.
    pub a_range: Range<usize>,
    /// The positions of `b` the segment's steps consume (empty if it only
    /// gaps `a`).
    pub b_range: Range<usize>,
    /// The segment's steps, as a range into the alignment's `steps`.
    pub steps: Range<usize>,
    /// The sum of its steps' scores.
    pub score: i64,
    /// What it cost to start this segment (run open, move, jump).
    pub open_cost: i64,
    pub placement: Placement,
}

/// What an aligner produces: the raw score and its explanation.
///
/// Invariants: `steps` are exactly the segments' steps, segments in `a`
/// order; `Σ steps.score == Σ segments.score`; `raw = Σ segments.score −
/// Σ segments.open_cost − reuse_penalty × reused`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawAlignment {
    pub raw: i64,
    pub steps: Vec<EchoStep>,
    pub segments: Vec<EchoSegment>,
    /// How many pairs each phoneme of `a` takes part in.
    pub a_coverage: Vec<u8>,
    /// How many pairs each phoneme of `b` takes part in.
    pub b_coverage: Vec<u8>,
}

impl RawAlignment {
    /// An alignment of nothing, for sequences of length `n` and `m`.
    pub fn empty(n: usize, m: usize) -> Self {
        Self {
            raw: 0,
            steps: Vec::new(),
            segments: Vec::new(),
            a_coverage: vec![0; n],
            b_coverage: vec![0; m],
        }
    }

    /// Extra uses of `a` phonemes beyond the first.
    pub fn reused_a(&self) -> usize {
        reused(&self.a_coverage)
    }

    /// Extra uses of `b` phonemes beyond the first.
    pub fn reused_b(&self) -> usize {
        reused(&self.b_coverage)
    }

    /// The same alignment with the roles of `a` and `b` exchanged, its
    /// segments re-sorted into the new `a` order and re-marked in place.
    pub fn transpose(self) -> Self {
        let mut segments: Vec<(EchoSegment, Vec<EchoStep>)> = self
            .segments
            .into_iter()
            .map(|segment| {
                let steps = self.steps[segment.steps.clone()]
                    .iter()
                    .map(|step| EchoStep {
                        step: swap_sides(&step.step),
                        a_index: step.b_index,
                        b_index: step.a_index,
                        score: step.score,
                    })
                    .collect();
                let segment = EchoSegment {
                    a_range: segment.b_range,
                    b_range: segment.a_range,
                    ..segment
                };
                (segment, steps)
            })
            .collect();
        segments.sort_by_key(|(segment, _)| (segment.a_range.start, segment.b_range.start));
        let (mut segments, steps) = concatenate(segments);
        mark_in_place(&mut segments);
        Self {
            raw: self.raw,
            steps,
            segments,
            a_coverage: self.b_coverage,
            b_coverage: self.a_coverage,
        }
    }
}

fn reused(coverage: &[u8]) -> usize {
    coverage
        .iter()
        .map(|&uses| usize::from(uses.saturating_sub(1)))
        .sum()
}

fn swap_sides(step: &OwnedAlignmentStep) -> OwnedAlignmentStep {
    match step.clone() {
        OwnedAlignmentStep::Match { a, b } => OwnedAlignmentStep::Match { a: b, b: a },
        OwnedAlignmentStep::Substitution { a, b } => {
            OwnedAlignmentStep::Substitution { a: b, b: a }
        }
        OwnedAlignmentStep::Indel { a, b } => OwnedAlignmentStep::Indel { a: b, b: a },
        OwnedAlignmentStep::Transposition { a, b, direction } => {
            OwnedAlignmentStep::Transposition {
                a: b,
                b: a,
                direction,
            }
        }
    }
}

/// An echo alignment strategy. Implementations differ in how they let
/// material go unaligned and how they treat re-ordering.
pub trait EchoAligner {
    fn kind(&self) -> AlignerKind;

    /// Whether `align(a, b)` and `align(b, a)` score the same. When not, the
    /// detector runs both orientations and keeps the better.
    fn is_symmetric(&self) -> bool {
        true
    }

    fn align(
        &self,
        a: &[Phoneme],
        b: &[Phoneme],
        table: &ScoreTable,
        costs: &EchoCosts,
    ) -> RawAlignment;
}

/// A scored, normalized echo comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoAlignment {
    pub raw: i64,
    /// `raw` over the normalization's denominator; 1 for a verbatim repeat,
    /// around 0 for unrelated lines. Unclamped: it can dip below 0, and
    /// exceed 1 when the jump aligner reuses material.
    pub similarity: f32,
    pub self_similarity_a: i64,
    pub self_similarity_b: i64,
    pub normalization: EchoNormalization,
    pub steps: Vec<EchoStep>,
    pub segments: Vec<EchoSegment>,
    pub a_coverage: Vec<u8>,
    pub b_coverage: Vec<u8>,
}

impl EchoAlignment {
    pub fn distance(&self) -> f32 {
        1.0 - self.similarity
    }

    pub fn moved_segments(&self) -> impl Iterator<Item = &EchoSegment> {
        self.segments
            .iter()
            .filter(|segment| segment.placement == Placement::Moved)
    }

    /// Extra uses of `a` phonemes beyond the first.
    pub fn reused_a(&self) -> usize {
        reused(&self.a_coverage)
    }

    /// Extra uses of `b` phonemes beyond the first.
    pub fn reused_b(&self) -> usize {
        reused(&self.b_coverage)
    }
}

/// Labels the heaviest chain of segments that is in order in both lines
/// [`Placement::InPlace`] and the rest [`Placement::Moved`]: a weighted
/// longest increasing subsequence over `b` positions, taking `segments` in
/// their (`a`) order and weighting each by its score.
pub fn mark_in_place(segments: &mut [EchoSegment]) {
    let weights: Vec<i64> = segments.iter().map(|segment| segment.score).collect();
    mark_in_place_weighted(segments, &weights);
}

/// [`mark_in_place`] with explicit weights. Two segments chain when the
/// earlier one (in `a`) ends in `b` no later than the later one starts.
/// Ties prefer the chain ending earliest, then the earliest predecessor.
/// Returns whether any chain had positive weight (an empty or all-negative
/// list still marks its single heaviest segment in place).
pub(crate) fn mark_in_place_weighted(segments: &mut [EchoSegment], weights: &[i64]) -> bool {
    let k = segments.len();
    if k == 0 {
        return false;
    }
    // chain[x]: the best weight of a chain ending at x; prev[x] its
    // predecessor.
    let mut chain = vec![0i64; k];
    let mut prev = vec![None; k];
    for x in 0..k {
        let mut best: Option<(i64, usize)> = None;
        for y in 0..x {
            if segments[y].b_range.end <= segments[x].b_range.start
                && chain[y] > 0
                && best.is_none_or(|(weight, _)| chain[y] > weight)
            {
                best = Some((chain[y], y));
            }
        }
        chain[x] = weights[x] + best.map_or(0, |(weight, _)| weight);
        prev[x] = best.map(|(_, y)| y);
    }
    let mut end = 0;
    for x in 1..k {
        if chain[x] > chain[end] {
            end = x;
        }
    }
    for segment in segments.iter_mut() {
        segment.placement = Placement::Moved;
    }
    let mut at = Some(end);
    while let Some(x) = at {
        segments[x].placement = Placement::InPlace;
        at = prev[x];
    }
    chain[end] > 0
}

/// Splits a trace into segments at its jumps and scores every step: pairs
/// from `table`, gaps from the per-position gap costs (open on a gap's first
/// phoneme). Segment ranges and step ranges are filled in; placement and
/// open costs are left to the aligner.
pub(crate) fn segments_from_trace(
    trace: &Trace,
    a: &[Phoneme],
    b: &[Phoneme],
    table: &ScoreTable,
    gaps_a: &GapScores,
    gaps_b: &GapScores,
) -> Vec<(EchoSegment, Vec<EchoStep>)> {
    let indexed = trace.indexed_steps(a, b);
    let mut runs: Vec<Vec<EchoStep>> = vec![Vec::new()];
    let mut previous: Option<TraceEvent> = None;
    let mut steps = indexed.into_iter();
    for &event in &trace.events {
        let score = match event {
            TraceEvent::Pair { i, j } => table.score(&a[i], &b[j]),
            TraceEvent::GapA { i } => {
                let opens = !matches!(previous, Some(TraceEvent::GapA { .. }));
                -(if opens { gaps_a.open[i] } else { 0 }) - gaps_a.extend[i]
            }
            TraceEvent::GapB { j } => {
                let opens = !matches!(previous, Some(TraceEvent::GapB { .. }));
                -(if opens { gaps_b.open[j] } else { 0 }) - gaps_b.extend[j]
            }
            TraceEvent::Jump => {
                runs.push(Vec::new());
                previous = Some(event);
                continue;
            }
            TraceEvent::Swap { .. } => unreachable!("echo aligners never swap"),
        };
        let indexed = steps.next().expect("one indexed step per non-jump event");
        runs.last_mut().expect("at least one run").push(EchoStep {
            step: indexed.step.into(),
            a_index: indexed.a_index,
            b_index: indexed.b_index,
            score,
        });
        previous = Some(event);
    }
    runs.into_iter()
        .filter(|steps| !steps.is_empty())
        .map(|steps| (segment_of(&steps), steps))
        .collect()
}

/// A segment spanning `steps` (step range left at 0..len).
pub(crate) fn segment_of(steps: &[EchoStep]) -> EchoSegment {
    let span = |indices: &mut dyn Iterator<Item = usize>| -> Range<usize> {
        let mut range: Option<Range<usize>> = None;
        for index in indices {
            range = Some(match range {
                None => index..index + 1,
                Some(r) => r.start.min(index)..r.end.max(index + 1),
            });
        }
        range.unwrap_or(0..0)
    };
    EchoSegment {
        a_range: span(&mut steps.iter().filter_map(|s| s.a_index)),
        b_range: span(&mut steps.iter().filter_map(|s| s.b_index)),
        steps: 0..steps.len(),
        score: steps.iter().map(|s| s.score).sum(),
        open_cost: 0,
        placement: Placement::InPlace,
    }
}

/// Concatenates segments' steps in the given order, pointing each segment's
/// step range at its slice.
pub(crate) fn concatenate(
    segments: Vec<(EchoSegment, Vec<EchoStep>)>,
) -> (Vec<EchoSegment>, Vec<EchoStep>) {
    let mut all_steps = Vec::new();
    let mut all_segments = Vec::with_capacity(segments.len());
    for (mut segment, steps) in segments {
        let start = all_steps.len();
        all_steps.extend(steps);
        segment.steps = start..all_steps.len();
        all_segments.push(segment);
    }
    (all_segments, all_steps)
}

/// How many pairs each position takes part in, per side.
pub(crate) fn coverage(steps: &[EchoStep], n: usize, m: usize) -> (Vec<u8>, Vec<u8>) {
    let (mut a, mut b) = (vec![0u8; n], vec![0u8; m]);
    for step in steps {
        if let (Some(i), Some(j)) = (step.a_index, step.b_index) {
            a[i] = a[i].saturating_add(1);
            b[j] = b[j].saturating_add(1);
        }
    }
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(a: Range<usize>, b: Range<usize>, score: i64) -> EchoSegment {
        EchoSegment {
            a_range: a,
            b_range: b,
            steps: 0..0,
            score,
            open_cost: 0,
            placement: Placement::InPlace,
        }
    }

    fn placements(segments: &[EchoSegment]) -> Vec<Placement> {
        segments.iter().map(|s| s.placement).collect()
    }

    use Placement::{InPlace, Moved};

    #[test]
    fn in_order_segments_are_all_in_place() {
        let mut segments = [segment(0..3, 0..3, 5), segment(3..6, 4..7, 5)];
        mark_in_place(&mut segments);
        assert_eq!(placements(&segments), [InPlace, InPlace]);
    }

    #[test]
    fn the_lighter_side_of_a_swap_is_moved() {
        // a = X Y, b = Y X: one of them moved — the lighter one.
        let mut segments = [segment(0..3, 4..7, 9), segment(3..7, 0..4, 4)];
        mark_in_place(&mut segments);
        assert_eq!(placements(&segments), [InPlace, Moved]);
        let mut segments = [segment(0..3, 4..7, 3), segment(3..7, 0..4, 8)];
        mark_in_place(&mut segments);
        assert_eq!(placements(&segments), [Moved, InPlace]);
    }

    #[test]
    fn a_reversed_triple_keeps_one_in_place() {
        // a = X Y Z, b = Z Y X.
        let mut segments = [
            segment(0..2, 4..6, 5),
            segment(2..4, 2..4, 6),
            segment(4..6, 0..2, 5),
        ];
        mark_in_place(&mut segments);
        assert_eq!(placements(&segments), [Moved, InPlace, Moved]);
        // a = X Y Z, b = Z X Y: X and Y stay in order.
        let mut segments = [
            segment(0..2, 2..4, 5),
            segment(2..4, 4..6, 5),
            segment(4..6, 0..2, 6),
        ];
        mark_in_place(&mut segments);
        assert_eq!(placements(&segments), [InPlace, InPlace, Moved]);
    }

    #[test]
    fn overlapping_b_ranges_do_not_chain() {
        // Reuse: every segment against the same b region.
        let mut segments = [
            segment(0..2, 0..2, 5),
            segment(2..4, 0..2, 5),
            segment(4..6, 0..2, 5),
        ];
        mark_in_place(&mut segments);
        assert_eq!(placements(&segments), [InPlace, Moved, Moved]);
    }
}
