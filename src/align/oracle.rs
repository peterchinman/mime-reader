//! Exhaustive-recursion oracles for the alignment recurrences.
//!
//! Each oracle enumerates every edit sequence of its model directly, sharing
//! only the cost primitives with the code under test — no matrix, no
//! boundary initialization, no traceback. They validate that a DP faithfully
//! maximizes its recurrence, not that the recurrence is the right model.
//! Exponential, so only for tiny inputs.

use super::{GapModel, NEG_INF, ScoreTable};
use crate::phoneme::Phoneme;

fn prev(s: &[Phoneme], i: usize) -> Option<&Phoneme> {
    i.checked_sub(1).map(|k| &s[k])
}

/// The best global score of `a` vs `b` under the linear model: pairs score
/// `table`, each gapped phoneme pays `extend` (same-sequence context), and
/// an adjacent consonant swap pays `swap`.
pub(crate) fn linear_global(
    a: &[Phoneme],
    b: &[Phoneme],
    table: &ScoreTable,
    gaps: &GapModel,
    swap: Option<i64>,
) -> i64 {
    fn go(
        a: &[Phoneme],
        b: &[Phoneme],
        i: usize,
        j: usize,
        table: &ScoreTable,
        gaps: &GapModel,
        swap: Option<i64>,
    ) -> i64 {
        if i == a.len() && j == b.len() {
            return 0;
        }
        let mut best = NEG_INF;
        if i < a.len() {
            let cost = gaps.extend_cost(&a[i], prev(a, i));
            best = best.max(go(a, b, i + 1, j, table, gaps, swap) - cost);
        }
        if j < b.len() {
            let cost = gaps.extend_cost(&b[j], prev(b, j));
            best = best.max(go(a, b, i, j + 1, table, gaps, swap) - cost);
        }
        if i < a.len() && j < b.len() {
            best = best.max(go(a, b, i + 1, j + 1, table, gaps, swap) + table.score(&a[i], &b[j]));
        }
        if let Some(cost) = swap
            && i + 1 < a.len()
            && j + 1 < b.len()
            && matches!(
                (&a[i], &a[i + 1]),
                (Phoneme::Consonant(_), Phoneme::Consonant(_))
            )
            && a[i] == b[j + 1]
            && a[i + 1] == b[j]
        {
            best = best.max(go(a, b, i + 2, j + 2, table, gaps, swap) - cost);
        }
        best
    }
    go(a, b, 0, 0, table, gaps, swap)
}
