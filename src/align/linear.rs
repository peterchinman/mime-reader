//! The single-state global recurrence: substitutions, per-phoneme gaps, and
//! optional adjacent-consonant swaps (optimal string alignment). This is the
//! rhyme DP in score space.

use std::cell::Cell;

use super::{GapModel, NEG_INF, ScoreTable, Trace, TraceEvent, is_consonant_id, phoneme_id};
use crate::phoneme::Phoneme;

// Traceback codes, one per matrix cell.
const START: u8 = 0;
const DIAGONAL: u8 = 1;
const GAP_B: u8 = 2;
const GAP_A: u8 = 3;
const SWAP: u8 = 4;

/// Keeps the higher score; on a tie the lower `priority` wins.
#[inline]
fn choose(best: &mut (i64, u8, u8), score: i64, priority: u8, edit: u8) {
    if score > best.0 || (score == best.0 && priority < best.1) {
        *best = (score, priority, edit);
    }
}

/// Reusable buffers, so the hot rhyme path (one alignment per dictionary
/// word in a scan) doesn't allocate per call.
#[derive(Default)]
struct Workspace {
    a: Vec<u8>,
    b: Vec<u8>,
    extend_a: Vec<i64>,
    extend_b: Vec<i64>,
    score: Vec<i64>,
    edits: Vec<u8>,
}

thread_local! {
    static WORKSPACE: Cell<Workspace> = Cell::new(Workspace::default());
}

/// Fills `ids` and `extend` for `phonemes`: their [`phoneme_id`]s and the
/// per-phoneme gap costs, with `context` as the predecessor of phoneme 0.
fn prepare(
    phonemes: &[Phoneme],
    gaps: &GapModel,
    context: Option<&Phoneme>,
    ids: &mut Vec<u8>,
    extend: &mut Vec<i64>,
) {
    ids.clear();
    extend.clear();
    let mut prev = context;
    for phoneme in phonemes {
        ids.push(phoneme_id(phoneme));
        extend.push(gaps.extend_cost(phoneme, prev));
        prev = Some(phoneme);
    }
}

/// Globally aligns `a` with `b`, maximizing
/// `Σ table(pair) − Σ gap extend − Σ swap`.
///
/// `D[i][j]`, the best score of aligning `a[..i]` with `b[..j]`, is the max of
/// - `D[i-1][j-1] + table[a_i][b_j]` (pair),
/// - `D[i][j-1] − extend_b[j]` (gap in `a`: `b_j` unaligned),
/// - `D[i-1][j] − extend_a[i]` (gap in `b`: `a_i` unaligned),
/// - `D[i-2][j-2] − swap` when `a_{i-1}, a_i` are consonants with
///   `a_i = b_{j-1}` and `a_{i-1} = b_j`.
///
/// Gaps are linear — each gapped phoneme pays its own `extend`, in the
/// context of its predecessor in the same sequence (`context_a` /
/// `context_b` before phoneme 0) — so the gap model's `open` terms must be 0.
///
/// Ties go to the most structural explanation, so the traceback is
/// deterministic: a zero-scored pair (priority 0) < swap (1) < any other pair
/// (2) < gap in `a` (3) < gap in `b` (4). These are the rhyme DP's
/// `choose_edit` priorities; each cell compares alignments of the same
/// prefixes, so cost and score pick the same cell winners and the same
/// traceback.
///
/// OSA caveats (deliberate): unlike unrestricted Damerau-Levenshtein, no
/// substring is edited more than once, and the result is not a metric.
#[allow(clippy::too_many_arguments)]
pub(crate) fn align_global(
    a: &[Phoneme],
    b: &[Phoneme],
    table: &ScoreTable,
    gaps: &GapModel,
    context_a: Option<&Phoneme>,
    context_b: Option<&Phoneme>,
    swap: Option<i64>,
    trace: bool,
) -> Trace {
    debug_assert!(
        gaps.open == 0 && gaps.open_repeated_consonant == 0,
        "the linear recurrence has no gap-open term",
    );
    WORKSPACE.with(|cell| {
        // Taking the workspace (leaving an empty one) keeps this correct even
        // if it were ever re-entered: the inner call would just allocate.
        let mut ws = cell.take();
        prepare(a, gaps, context_a, &mut ws.a, &mut ws.extend_a);
        prepare(b, gaps, context_b, &mut ws.b, &mut ws.extend_b);
        let result = align_ids(
            &ws.a,
            &ws.b,
            table,
            &ws.extend_a,
            &ws.extend_b,
            swap,
            trace,
            &mut ws.score,
            &mut ws.edits,
        );
        cell.set(ws);
        result
    })
}

/// [`align_global`] on phoneme ids and per-position gap costs, with the
/// matrix in caller-provided buffers.
#[allow(clippy::too_many_arguments)]
fn align_ids(
    a: &[u8],
    b: &[u8],
    table: &ScoreTable,
    extend_a: &[i64],
    extend_b: &[i64],
    swap: Option<i64>,
    trace: bool,
    score: &mut Vec<i64>,
    edits: &mut Vec<u8>,
) -> Trace {
    let (n, m) = (a.len(), b.len());
    let width = m + 1;
    score.clear();
    score.resize((n + 1) * width, 0);
    edits.clear();
    if trace {
        edits.resize((n + 1) * width, START);
    }

    // Boundaries: a prefix aligned against nothing is the running sum of its
    // gap costs.
    for i in 1..=n {
        score[i * width] = score[(i - 1) * width] - extend_a[i - 1];
        if trace {
            edits[i * width] = GAP_A;
        }
    }
    for j in 1..=m {
        score[j] = score[j - 1] - extend_b[j - 1];
        if trace {
            edits[j] = GAP_B;
        }
    }

    for i in 1..=n {
        let a_i = a[i - 1];
        let gap_a = extend_a[i - 1];
        // a_{i-1}, a_i both consonants: the precondition for a swap ending here.
        let swappable =
            swap.is_some() && i >= 2 && is_consonant_id(a[i - 2]) && is_consonant_id(a_i);
        let row = i * width;
        let up = (i - 1) * width;
        for j in 1..=m {
            let b_j = b[j - 1];
            let pair = table.get(a_i, b_j);

            let mut best = (NEG_INF, u8::MAX, START);
            if swappable
                && j >= 2
                && a_i == b[j - 2]
                && a[i - 2] == b_j
                && let Some(swap_cost) = swap
            {
                choose(
                    &mut best,
                    score[(i - 2) * width + j - 2] - swap_cost,
                    1,
                    SWAP,
                );
            }
            choose(
                &mut best,
                score[up + j - 1] + pair,
                if pair == 0 { 0 } else { 2 },
                DIAGONAL,
            );
            choose(&mut best, score[row + j - 1] - extend_b[j - 1], 3, GAP_B);
            choose(&mut best, score[up + j] - gap_a, 4, GAP_A);

            score[row + j] = best.0;
            if trace {
                edits[row + j] = best.2;
            }
        }
    }

    let events = if trace {
        traceback(edits, n, m)
    } else {
        Vec::new()
    };
    Trace {
        score: score[n * width + m],
        events,
    }
}

fn traceback(edits: &[u8], n: usize, m: usize) -> Vec<TraceEvent> {
    let width = m + 1;
    let (mut i, mut j) = (n, m);
    let mut events = Vec::with_capacity(n + m);
    while i > 0 || j > 0 {
        match edits[i * width + j] {
            DIAGONAL => {
                i -= 1;
                j -= 1;
                events.push(TraceEvent::Pair { i, j });
            }
            GAP_B => {
                j -= 1;
                events.push(TraceEvent::GapB { j });
            }
            GAP_A => {
                i -= 1;
                events.push(TraceEvent::GapA { i });
            }
            SWAP => {
                i -= 2;
                j -= 2;
                events.push(TraceEvent::Swap { i, j });
            }
            _ => break,
        }
    }
    events.reverse();
    events
}

#[cfg(test)]
mod tests {
    use super::super::{GapScores, oracle};
    use super::*;

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    /// A small cost model on rhyme-like scales: vowels never pair with
    /// consonants, identical phonemes are free, near ones cheap.
    fn table() -> ScoreTable {
        ScoreTable::from_fn(|p, q| match (p, q) {
            _ if p == q => 0,
            (Phoneme::Vowel(_), Phoneme::Vowel(_)) => -300,
            (Phoneme::Consonant(_), Phoneme::Consonant(_)) => -400,
            _ => -100_000,
        })
    }

    const GAPS: GapModel = GapModel {
        open: 0,
        open_repeated_consonant: 0,
        extend_vowel: 5_000,
        extend_consonant: 500,
        extend_repeated_consonant: 100,
    };

    fn run(a: &[Phoneme], b: &[Phoneme], swap: Option<i64>, trace: bool) -> Trace {
        align_global(a, b, &table(), &GAPS, None, None, swap, trace)
    }

    /// Re-scores a traceback from its own events.
    fn rescore(trace: &Trace, a: &[Phoneme], b: &[Phoneme], swap: Option<i64>) -> i64 {
        let (ga, gb) = (
            GapScores::new(&GAPS, a, None),
            GapScores::new(&GAPS, b, None),
        );
        trace
            .events
            .iter()
            .map(|event| match *event {
                TraceEvent::Pair { i, j } => table().score(&a[i], &b[j]),
                TraceEvent::GapA { i } => -ga.extend[i],
                TraceEvent::GapB { j } => -gb.extend[j],
                TraceEvent::Swap { .. } => -swap.unwrap(),
                TraceEvent::Jump => unreachable!(),
            })
            .sum()
    }

    #[test]
    fn swap_is_taken_when_cheaper_than_two_substitutions() {
        let (a, b) = (parse("M OW1 S T"), parse("M OW1 T S"));
        let trace = run(&a, &b, Some(200), true);
        assert_eq!(trace.score, -200);
        assert_eq!(
            trace.events,
            [
                TraceEvent::Pair { i: 0, j: 0 },
                TraceEvent::Pair { i: 1, j: 1 },
                TraceEvent::Swap { i: 2, j: 2 },
            ]
        );
        // Without swaps the same pair costs two substitutions.
        assert_eq!(run(&a, &b, None, false).score, -800);
    }

    #[test]
    fn traced_score_equals_untraced_and_rescored() {
        let alphabet = parse("AE1 IH0 UW1 K T S P B");
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut rng = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as usize
        };
        for _ in 0..1000 {
            let a: Vec<Phoneme> = (0..rng() % 7)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            let b: Vec<Phoneme> = (0..rng() % 7)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            for swap in [None, Some(200)] {
                let traced = run(&a, &b, swap, true);
                assert_eq!(traced.score, run(&a, &b, swap, false).score);
                assert_eq!(
                    rescore(&traced, &a, &b, swap),
                    traced.score,
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn matches_the_exhaustive_oracle() {
        let alphabet = parse("AE1 IH0 OW1 K T S B B");
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rng = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as usize
        };
        let table = table();
        for _ in 0..500 {
            let a: Vec<Phoneme> = (0..rng() % 5)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            let b: Vec<Phoneme> = (0..rng() % 5)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            for swap in [None, Some(200)] {
                let expected = oracle::linear_global(&a, &b, &table, &GAPS, swap);
                assert_eq!(run(&a, &b, swap, false).score, expected, "{a:?} vs {b:?}");
            }
        }
    }
}
