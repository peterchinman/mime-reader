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

/// What the previous event of a partial alignment was: it decides whether a
/// gap opens or extends, and whether the alignment may stop here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    Start,
    Pair,
    GapA,
    GapB,
}

/// The best score of `a` vs `b` under the affine model in `mode`, by
/// enumerating every alignment directly from its definition:
///
/// - Global: start at (0, 0), consume everything.
/// - SemiGlobal: start at the start of `a` or of `b`; stop on a pair (or
///   before any) once either sequence is exhausted.
/// - Local: start on any pair; stop after any pair; or align nothing (0).
///
/// Masked rows/columns may not be paired or gapped, nor gapped across.
pub(crate) fn affine(
    a: &[Phoneme],
    b: &[Phoneme],
    table: &ScoreTable,
    gaps: &GapModel,
    mode: super::affine::Mode,
    mask: Option<&super::affine::Mask>,
) -> i64 {
    use super::affine::Mode;
    use std::collections::HashMap;

    struct Search<'s> {
        a: &'s [Phoneme],
        b: &'s [Phoneme],
        table: &'s ScoreTable,
        gaps: &'s GapModel,
        mode: Mode,
        mask: Option<&'s super::affine::Mask>,
        memo: HashMap<(usize, usize, u8), i64>,
    }

    impl Search<'_> {
        fn row_ok(&self, i: usize) -> bool {
            self.mask.is_none_or(|mask| !mask.rows[i])
        }
        fn col_ok(&self, j: usize) -> bool {
            self.mask.is_none_or(|mask| !mask.cols[j])
        }
        fn pair(&self, i: usize, j: usize) -> Option<i64> {
            (self.row_ok(i) && self.col_ok(j)).then(|| self.table.score(&self.a[i], &self.b[j]))
        }

        fn go(&mut self, i: usize, j: usize, last: Last) -> i64 {
            let key = (i, j, last as u8);
            if let Some(&known) = self.memo.get(&key) {
                return known;
            }
            let (n, m) = (self.a.len(), self.b.len());
            let mut best = NEG_INF;
            let may_stop = match self.mode {
                Mode::Global => i == n && j == m,
                Mode::SemiGlobal => matches!(last, Last::Start | Last::Pair) && (i == n || j == m),
                Mode::Local => last == Last::Pair,
            };
            if may_stop {
                best = 0;
            }
            if i < n
                && j < m
                && let Some(score) = self.pair(i, j)
            {
                best = best.max((score + self.go(i + 1, j + 1, Last::Pair)).max(NEG_INF));
            }
            if i < n && self.row_ok(i) && (j == 0 || self.col_ok(j - 1)) {
                let open = if last == Last::GapA {
                    0
                } else {
                    self.gaps.open_cost(&self.a[i], prev(self.a, i))
                };
                let cost = open + self.gaps.extend_cost(&self.a[i], prev(self.a, i));
                best = best.max((self.go(i + 1, j, Last::GapA) - cost).max(NEG_INF));
            }
            if j < m && self.col_ok(j) && (i == 0 || self.row_ok(i - 1)) {
                let open = if last == Last::GapB {
                    0
                } else {
                    self.gaps.open_cost(&self.b[j], prev(self.b, j))
                };
                let cost = open + self.gaps.extend_cost(&self.b[j], prev(self.b, j));
                best = best.max((self.go(i, j + 1, Last::GapB) - cost).max(NEG_INF));
            }
            self.memo.insert(key, best);
            best
        }
    }

    let mut search = Search {
        a,
        b,
        table,
        gaps,
        mode,
        mask,
        memo: HashMap::new(),
    };
    let (n, m) = (a.len(), b.len());
    match mode {
        Mode::Global => search.go(0, 0, Last::Start),
        Mode::SemiGlobal => {
            let mut best = NEG_INF;
            for i in 0..=n {
                best = best.max(search.go(i, 0, Last::Start));
            }
            for j in 0..=m {
                best = best.max(search.go(0, j, Last::Start));
            }
            best
        }
        Mode::Local => {
            let mut best = 0;
            for i in 0..n {
                for j in 0..m {
                    if let Some(score) = search.pair(i, j) {
                        best = best.max(score + search.go(i + 1, j + 1, Last::Pair));
                    }
                }
            }
            best
        }
    }
}
