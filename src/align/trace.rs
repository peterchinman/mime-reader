//! What a traceback emits, and how it becomes alignment steps.

use super::{AlignmentStep, OwnedAlignmentStep, TranspositionDirection};
use crate::phoneme::Phoneme;

/// One event of a traceback, in alignment order. Indices are 0-based
/// positions in the aligned sequences `a` and `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceEvent {
    /// `a[i]` aligned with `b[j]`.
    Pair { i: usize, j: usize },
    /// `a[i]` aligned with nothing.
    GapA { i: usize },
    /// `b[j]` aligned with nothing.
    GapB { j: usize },
    /// Adjacent swap: `a[i], a[i+1]` crossed with `b[j], b[j+1]`
    /// (`a[i] = b[j+1]` and `a[i+1] = b[j]`).
    Swap { i: usize, j: usize },
    /// The alignment resumes somewhere else in `b`: the pairs after this
    /// event don't continue the diagonal of the pairs before it.
    Jump,
}

/// A scored traceback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub score: i64,
    /// Empty unless the traceback was requested.
    pub events: Vec<TraceEvent>,
}

/// One alignment step with the positions it consumed in each sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexedStep<'a> {
    pub step: AlignmentStep<'a>,
    pub a_index: Option<usize>,
    pub b_index: Option<usize>,
}

impl Trace {
    /// The trace as display steps: a pair is a `Match` when the phonemes are
    /// equal and a `Substitution` otherwise; a swap is a `Right` then a
    /// `Left` transposition; jumps emit nothing.
    pub fn steps<'a>(&self, a: &'a [Phoneme], b: &'a [Phoneme]) -> Vec<AlignmentStep<'a>> {
        self.indexed_steps(a, b)
            .into_iter()
            .map(|indexed| indexed.step)
            .collect()
    }

    /// [`Self::steps`] as owned steps.
    pub fn owned_steps(&self, a: &[Phoneme], b: &[Phoneme]) -> Vec<OwnedAlignmentStep> {
        self.indexed_steps(a, b)
            .into_iter()
            .map(|indexed| OwnedAlignmentStep::from(indexed.step))
            .collect()
    }

    /// [`Self::steps`], each with the positions it consumed.
    pub fn indexed_steps<'a>(&self, a: &'a [Phoneme], b: &'a [Phoneme]) -> Vec<IndexedStep<'a>> {
        let mut steps = Vec::with_capacity(self.events.len());
        for &event in &self.events {
            match event {
                TraceEvent::Pair { i, j } => {
                    let step = if a[i] == b[j] {
                        AlignmentStep::Match { a: &a[i], b: &b[j] }
                    } else {
                        AlignmentStep::Substitution { a: &a[i], b: &b[j] }
                    };
                    steps.push(IndexedStep {
                        step,
                        a_index: Some(i),
                        b_index: Some(j),
                    });
                }
                TraceEvent::GapA { i } => steps.push(IndexedStep {
                    step: AlignmentStep::Indel {
                        a: Some(&a[i]),
                        b: None,
                    },
                    a_index: Some(i),
                    b_index: None,
                }),
                TraceEvent::GapB { j } => steps.push(IndexedStep {
                    step: AlignmentStep::Indel {
                        a: None,
                        b: Some(&b[j]),
                    },
                    a_index: None,
                    b_index: Some(j),
                }),
                TraceEvent::Swap { i, j } => {
                    steps.push(IndexedStep {
                        step: AlignmentStep::Transposition {
                            a: &a[i],
                            b: &b[j],
                            direction: TranspositionDirection::Right,
                        },
                        a_index: Some(i),
                        b_index: Some(j),
                    });
                    steps.push(IndexedStep {
                        step: AlignmentStep::Transposition {
                            a: &a[i + 1],
                            b: &b[j + 1],
                            direction: TranspositionDirection::Left,
                        },
                        a_index: Some(i + 1),
                        b_index: Some(j + 1),
                    });
                }
                TraceEvent::Jump => {}
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    #[test]
    fn events_become_steps_in_order() {
        let a = parse("M OW1 S T AE1");
        let b = parse("M AO1 T S IH0 K");
        let trace = Trace {
            score: 0,
            events: vec![
                TraceEvent::Pair { i: 0, j: 0 },
                TraceEvent::Pair { i: 1, j: 1 },
                TraceEvent::Swap { i: 2, j: 2 },
                TraceEvent::Jump,
                TraceEvent::GapA { i: 4 },
                TraceEvent::GapB { j: 4 },
                TraceEvent::GapB { j: 5 },
            ],
        };
        let steps = trace.indexed_steps(&a, &b);
        let shapes: Vec<_> = steps
            .iter()
            .map(|s| match s.step {
                AlignmentStep::Match { .. } => "match",
                AlignmentStep::Substitution { .. } => "sub",
                AlignmentStep::Indel { a: Some(_), .. } => "gap_a",
                AlignmentStep::Indel { .. } => "gap_b",
                AlignmentStep::Transposition {
                    direction: TranspositionDirection::Right,
                    ..
                } => "right",
                AlignmentStep::Transposition { .. } => "left",
            })
            .collect();
        assert_eq!(
            shapes,
            ["match", "sub", "right", "left", "gap_a", "gap_b", "gap_b"]
        );
        assert_eq!(steps[2].a_index, Some(2));
        assert_eq!(steps[3].b_index, Some(3));
        assert_eq!((steps[4].a_index, steps[4].b_index), (Some(4), None));
        assert_eq!((steps[6].a_index, steps[6].b_index), (None, Some(5)));
        // The Right transposition carries the earlier phonemes of each side.
        assert_eq!(
            trace.owned_steps(&a, &b)[2],
            OwnedAlignmentStep::Transposition {
                a: a[2].clone(),
                b: b[2].clone(),
                direction: TranspositionDirection::Right,
            }
        );
    }
}
