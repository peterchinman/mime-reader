//! The three-state Gotoh recurrence: affine gaps (`open + Σ extend`), in
//! global, semi-global and local modes, with optional row/column masks. Echo's aligners are all built on it.
//!
//! States, for `a[..i]` vs `b[..j]`:
//! - `M[i][j]`: the alignment ends pairing `a_i` with `b_j`;
//! - `X[i][j]`: it ends skipping `a_i`;
//! - `Y[i][j]`: it ends skipping `b_j`.
//!
//! With `go(i)` / `ge(i)` the open / extend cost at position `i` of its own
//! sequence ([`GapScores`]):
//!
//! ```text
//! M[i][j] = s(a_i, b_j) + entry(i, j)
//! X[i][j] = max(M[i-1][j] − go(i) − ge(i), X[i-1][j] − ge(i), Y[i-1][j] − go(i) − ge(i))
//! Y[i][j] = max(M[i][j-1] − go(j) − ge(j), Y[i][j-1] − ge(j), X[i][j-1] − go(j) − ge(j))
//! ```
//!
//! where `entry` is what a pair may follow, per [`Mode`]. Values saturate at
//! [`NEG_INF`], so impossible states (masked cells, cross-class pairs scored
//! `NEG_INF`) never overflow and never beat a possible one.

use super::{GapScores, NEG_INF, ScoreTable, Trace, TraceEvent};

/// Where alignments may start and end, and what a pair may follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Both sequences aligned end to end (leading and trailing gaps cost).
    /// Echo doesn't use it; it anchors the cross-check against the linear
    /// recurrence.
    #[allow(dead_code)]
    Global,
    /// Free end gaps: the alignment starts at the start of `a` or of `b` and
    /// ends at the end of `a` or of `b`, so a prefix of one and a suffix of
    /// either can go unaligned for free. Ends on a pair (or aligns nothing,
    /// scoring 0).
    SemiGlobal,
    /// Smith–Waterman: the best-scoring contiguous region of each, starting
    /// and ending on a pair (or nothing, scoring 0).
    Local,
}

/// Positions that may not take part in an alignment: a masked row `i`
/// walls off `a_i`, a masked column `j` walls off `b_j` — all three states,
/// so no pair or gap can consume them and no alignment can pass through
/// them. Masking a local run's full ranges therefore keeps later runs
/// contiguous and disjoint from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mask {
    /// `rows[i]`: `a[i]` is masked.
    pub rows: Vec<bool>,
    /// `cols[j]`: `b[j]` is masked.
    pub cols: Vec<bool>,
}

impl Mask {
    /// No positions masked, for sequences of length `n` and `m`.
    pub fn new(n: usize, m: usize) -> Self {
        Self {
            rows: vec![false; n],
            cols: vec![false; m],
        }
    }

    /// Masks `a[rows]` and `b[cols]`.
    pub fn block(&mut self, rows: std::ops::Range<usize>, cols: std::ops::Range<usize>) {
        self.rows[rows].fill(true);
        self.cols[cols].fill(true);
    }
}

// The three states, also used as traceback pointers into them.
const STATE_M: u8 = 0;
const STATE_X: u8 = 1;
const STATE_Y: u8 = 2;
// Extra `M` predecessors.
/// A border start: nothing precedes this cell.
const START: u8 = 3;
/// Local mode: the alignment starts fresh at this pair.
const RESET: u8 = 4;

/// Saturates at [`NEG_INF`]: anything at or below it is "impossible".
#[inline]
fn floor(score: i64) -> i64 {
    score.max(NEG_INF)
}

/// The best of the three states at one cell; ties prefer `M`, then `X`.
#[inline]
fn best3(m: i64, x: i64, y: i64) -> (i64, u8) {
    let mut best = (m, STATE_M);
    if x > best.0 {
        best = (x, STATE_X);
    }
    if y > best.0 {
        best = (y, STATE_Y);
    }
    best
}

/// Aligns `a` with `b` (phoneme ids) under `mode`, maximizing pair scores
/// minus affine gap costs.
///
/// Ties prefer, at every cell: `M` over `X` over `Y`; and in local mode a fresh start over continuing an
/// alignment that hasn't gained anything (score ≤ 0). The end cell prefers
/// `(n, m)`; local mode takes the first best cell in row-major order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn align(
    a: &[u8],
    b: &[u8],
    table: &ScoreTable,
    gaps_a: &GapScores,
    gaps_b: &GapScores,
    mode: Mode,
    mask: Option<&Mask>,
    trace: bool,
) -> Trace {
    let (n, m) = (a.len(), b.len());
    debug_assert_eq!(gaps_a.len(), n);
    debug_assert_eq!(gaps_b.len(), m);
    let width = m + 1;
    let cells = (n + 1) * width;
    let at = |i: usize, j: usize| i * width + j;
    let row_masked = |i: usize| mask.is_some_and(|mask| mask.rows[i - 1]);
    let col_masked = |j: usize| mask.is_some_and(|mask| mask.cols[j - 1]);

    let mut sm = vec![NEG_INF; cells];
    let mut sx = vec![NEG_INF; cells];
    let mut sy = vec![NEG_INF; cells];
    let (mut pm, mut px, mut py) = if trace {
        (vec![START; cells], vec![START; cells], vec![START; cells])
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    // Borders.
    match mode {
        Mode::Global => {
            // A border skips a whole prefix, so it ends at the first masked
            // position (the rest stays NEG_INF).
            sm[at(0, 0)] = 0;
            for i in (1..=n).take_while(|&i| !row_masked(i)) {
                let (score, from) = x_entry(&sm, &sx, &sy, at(i - 1, 0), gaps_a, i);
                sx[at(i, 0)] = score;
                if trace {
                    px[at(i, 0)] = from;
                }
            }
            for j in (1..=m).take_while(|&j| !col_masked(j)) {
                let (score, from) = y_entry(&sm, &sx, &sy, at(0, j - 1), gaps_b, j);
                sy[at(0, j)] = score;
                if trace {
                    py[at(0, j)] = from;
                }
            }
        }
        Mode::SemiGlobal => {
            for i in 0..=n {
                sm[at(i, 0)] = 0;
            }
            for j in 0..=m {
                sm[at(0, j)] = 0;
            }
        }
        Mode::Local => {}
    }

    for i in 1..=n {
        for j in 1..=m {
            let here = at(i, j);
            if row_masked(i) || col_masked(j) {
                continue; // stays NEG_INF in all three states
            }
            let diag = at(i - 1, j - 1);
            let (entry, from) = best3(sm[diag], sx[diag], sy[diag]);
            let (entry, from) = match mode {
                Mode::Global | Mode::SemiGlobal => (entry, from),
                Mode::Local => {
                    if entry > 0 {
                        (entry, from)
                    } else {
                        (0, RESET)
                    }
                }
            };
            sm[here] = floor(table.get(a[i - 1], b[j - 1]) + entry);

            let (x, x_from) = x_entry(&sm, &sx, &sy, at(i - 1, j), gaps_a, i);
            sx[here] = x;
            let (y, y_from) = y_entry(&sm, &sx, &sy, at(i, j - 1), gaps_b, j);
            sy[here] = y;

            if trace {
                pm[here] = from;
                px[here] = x_from;
                py[here] = y_from;
            }
        }
    }

    // The end cell.
    let end: Option<(i64, usize, usize, u8)> = match mode {
        Mode::Global => {
            let (score, state) = best3(sm[at(n, m)], sx[at(n, m)], sy[at(n, m)]);
            Some((score, n, m, state))
        }
        Mode::SemiGlobal => {
            let mut best = (sm[at(n, m)], n, m, STATE_M);
            for j in (0..m).rev() {
                if sm[at(n, j)] > best.0 {
                    best = (sm[at(n, j)], n, j, STATE_M);
                }
            }
            for i in (0..n).rev() {
                if sm[at(i, m)] > best.0 {
                    best = (sm[at(i, m)], i, m, STATE_M);
                }
            }
            Some(best)
        }
        Mode::Local => {
            let mut best: Option<(i64, usize, usize, u8)> = None;
            for i in 1..=n {
                for j in 1..=m {
                    let score = sm[at(i, j)];
                    if score > 0 && best.is_none_or(|(b, ..)| score > b) {
                        best = Some((score, i, j, STATE_M));
                    }
                }
            }
            best
        }
    };

    let Some((score, end_i, end_j, end_state)) = end else {
        return Trace {
            score: 0,
            events: Vec::new(),
        };
    };
    // Unreachable (only possible in global mode, when masks wall off every
    // path): there is no alignment to trace.
    let events = if trace && score > NEG_INF / 2 {
        traceback((end_i, end_j, end_state), width, &pm, &px, &py)
    } else {
        Vec::new()
    };
    Trace { score, events }
}

/// `X` at `(i, ·)` from the cell above, `up`: open after `M` or `Y`, extend
/// after `X`.
#[inline]
fn x_entry(sm: &[i64], sx: &[i64], sy: &[i64], up: usize, gaps: &GapScores, i: usize) -> (i64, u8) {
    let (open, extend) = (gaps.open[i - 1], gaps.extend[i - 1]);
    let mut best = (sm[up] - open - extend, STATE_M);
    let from_x = sx[up] - extend;
    if from_x > best.0 {
        best = (from_x, STATE_X);
    }
    let from_y = sy[up] - open - extend;
    if from_y > best.0 {
        best = (from_y, STATE_Y);
    }
    (floor(best.0), best.1)
}

/// `Y` at `(·, j)` from the cell to the left, `left`: open after `M` or `X`,
/// extend after `Y`.
#[inline]
fn y_entry(
    sm: &[i64],
    sx: &[i64],
    sy: &[i64],
    left: usize,
    gaps: &GapScores,
    j: usize,
) -> (i64, u8) {
    let (open, extend) = (gaps.open[j - 1], gaps.extend[j - 1]);
    let mut best = (sm[left] - open - extend, STATE_M);
    let from_x = sx[left] - open - extend;
    if from_x > best.0 {
        best = (from_x, STATE_X);
    }
    let from_y = sy[left] - extend;
    if from_y > best.0 {
        best = (from_y, STATE_Y);
    }
    (floor(best.0), best.1)
}

fn traceback(
    end: (usize, usize, u8),
    width: usize,
    pm: &[u8],
    px: &[u8],
    py: &[u8],
) -> Vec<TraceEvent> {
    let (mut i, mut j, mut state) = end;
    let mut events = Vec::new();
    loop {
        let here = i * width + j;
        match state {
            STATE_M => {
                if i == 0 || j == 0 {
                    break; // a border start
                }
                events.push(TraceEvent::Pair { i: i - 1, j: j - 1 });
                match pm[here] {
                    RESET | START => break,
                    from => {
                        i -= 1;
                        j -= 1;
                        state = from;
                    }
                }
            }
            STATE_X => {
                events.push(TraceEvent::SkipA { i: i - 1 });
                state = px[here];
                i -= 1;
            }
            STATE_Y => {
                events.push(TraceEvent::SkipB { j: j - 1 });
                state = py[here];
                j -= 1;
            }
            _ => break,
        }
    }
    events.reverse();
    events
}

#[cfg(test)]
mod tests {
    use super::super::{GapModel, linear, oracle, phoneme_ids};
    use super::*;
    use crate::phoneme::Phoneme;

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    /// A similarity table: alike phonemes positive, unlike negative, and
    /// vowels never pair with consonants.
    fn table() -> ScoreTable {
        ScoreTable::from_fn(|p, q| match (p, q) {
            (Phoneme::Vowel(_), Phoneme::Vowel(_)) if p == q => 40,
            (Phoneme::Consonant(_), Phoneme::Consonant(_)) if p == q => 22,
            (Phoneme::Vowel(_), Phoneme::Vowel(_)) => 5,
            (Phoneme::Consonant(_), Phoneme::Consonant(_)) => -9,
            _ => NEG_INF,
        })
    }

    const GAPS: GapModel = GapModel {
        open: 20,
        open_repeated_consonant: 10,
        extend_vowel: 3,
        extend_consonant: 1,
        extend_repeated_consonant: 0,
    };

    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
        fn sequence(&mut self, alphabet: &[Phoneme], max_len: usize) -> Vec<Phoneme> {
            (0..self.below(max_len + 1))
                .map(|_| alphabet[self.below(alphabet.len())].clone())
                .collect()
        }
        fn mask(&mut self, n: usize, m: usize) -> Mask {
            Mask {
                rows: (0..n).map(|_| self.below(4) == 0).collect(),
                cols: (0..m).map(|_| self.below(4) == 0).collect(),
            }
        }
    }

    fn run(
        a: &[Phoneme],
        b: &[Phoneme],
        mode: Mode,
        mask: Option<&Mask>,
        gaps: &GapModel,
    ) -> Trace {
        align(
            &phoneme_ids(a),
            &phoneme_ids(b),
            &table(),
            &GapScores::new(gaps, a, None),
            &GapScores::new(gaps, b, None),
            mode,
            mask,
            true,
        )
    }

    /// Re-scores a traceback from its events alone: pair scores plus gap
    /// open (when a gap of that side starts) plus extend.
    fn rescore(trace: &Trace, a: &[Phoneme], b: &[Phoneme], gaps: &GapModel) -> i64 {
        let (ga, gb) = (GapScores::new(gaps, a, None), GapScores::new(gaps, b, None));
        let mut total = 0;
        let mut previous: Option<TraceEvent> = None;
        for &event in &trace.events {
            total += match event {
                TraceEvent::Pair { i, j } => table().score(&a[i], &b[j]),
                TraceEvent::SkipA { i } => {
                    let open = !matches!(previous, Some(TraceEvent::SkipA { .. }));
                    -(if open { ga.open[i] } else { 0 }) - ga.extend[i]
                }
                TraceEvent::SkipB { j } => {
                    let open = !matches!(previous, Some(TraceEvent::SkipB { .. }));
                    -(if open { gb.open[j] } else { 0 }) - gb.extend[j]
                }
                TraceEvent::Swap { .. } => panic!("affine never swaps"),
            };
            previous = Some(event);
        }
        total
    }

    /// The events consume `a` and `b` in order, never touching masked
    /// positions, and satisfy the mode's coverage rules.
    fn assert_well_formed(trace: &Trace, n: usize, m: usize, mode: Mode, mask: Option<&Mask>) {
        let mut next_i = None;
        let mut next_j = None;
        let mut a_seen = vec![false; n];
        for &event in &trace.events {
            let (i, j) = match event {
                TraceEvent::Pair { i, j } => (Some(i), Some(j)),
                TraceEvent::SkipA { i } => (Some(i), None),
                TraceEvent::SkipB { j } => (None, Some(j)),
                TraceEvent::Swap { .. } => unreachable!(),
            };
            if let Some(i) = i {
                assert!(
                    next_i.is_none_or(|k| i == k),
                    "a out of order: {:?}",
                    trace.events
                );
                assert!(mask.is_none_or(|mask| !mask.rows[i]), "masked row {i} used");
                a_seen[i] = true;
                next_i = Some(i + 1);
            }
            if let Some(j) = j {
                assert!(
                    next_j.is_none_or(|k| j == k),
                    "b out of order: {:?}",
                    trace.events
                );
                assert!(
                    mask.is_none_or(|mask| !mask.cols[j]),
                    "masked column {j} used"
                );
                next_j = Some(j + 1);
            }
        }
        match mode {
            Mode::Global => {
                assert!(a_seen.iter().all(|&seen| seen), "all of a must be consumed")
            }
            Mode::SemiGlobal | Mode::Local => {
                if !matches!(mode, Mode::SemiGlobal) || trace.events.is_empty() {
                    return;
                }
                // Semi-global: starts at the start of a or of b, ends on a
                // pair at the end of a or of b.
                let first_pair = trace.events.iter().find_map(|e| match *e {
                    TraceEvent::Pair { i, j } => Some((i, j)),
                    _ => None,
                });
                let last_pair = trace.events.iter().rev().find_map(|e| match *e {
                    TraceEvent::Pair { i, j } => Some((i, j)),
                    _ => None,
                });
                if let (Some(_), Some((li, lj))) = (first_pair, last_pair) {
                    assert!(
                        li + 1 == n || lj + 1 == m,
                        "semi-global must end at an edge"
                    );
                }
            }
        }
    }

    fn modes() -> [Mode; 3] {
        [Mode::Global, Mode::SemiGlobal, Mode::Local]
    }

    #[test]
    fn matches_the_exhaustive_oracle_in_every_mode() {
        let alphabet = parse("AE1 IH0 OW1 K T T S");
        let table = table();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..600 {
            let a = rng.sequence(&alphabet, 4);
            let b = rng.sequence(&alphabet, 4);
            for gaps in [
                GAPS,
                GapModel {
                    open: 0,
                    open_repeated_consonant: 0,
                    ..GAPS
                },
            ] {
                for mode in modes() {
                    let expected = oracle::affine(&a, &b, &table, &gaps, mode, None);
                    let trace = run(&a, &b, mode, None, &gaps);
                    assert_eq!(trace.score, expected, "{mode:?} {a:?} vs {b:?}");
                    assert_eq!(
                        rescore(&trace, &a, &b, &gaps),
                        trace.score,
                        "{mode:?} {a:?} vs {b:?}: {:?}",
                        trace.events
                    );
                    assert_well_formed(&trace, a.len(), b.len(), mode, None);
                }
            }
        }
    }

    #[test]
    fn masked_alignment_matches_the_oracle_and_avoids_the_mask_in_every_mode() {
        let alphabet = parse("AE1 IH0 OW1 K T S");
        let table = table();
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        for _ in 0..800 {
            let a = rng.sequence(&alphabet, 4);
            let b = rng.sequence(&alphabet, 4);
            let mask = rng.mask(a.len(), b.len());
            for mode in modes() {
                let context = || format!("{mode:?} {a:?} vs {b:?} masked {mask:?}");
                let expected = oracle::affine(&a, &b, &table, &GAPS, mode, Some(&mask));
                let trace = run(&a, &b, mode, Some(&mask), &GAPS);
                if expected <= NEG_INF / 2 {
                    // Walled off (global mode): no alignment, nothing traced.
                    assert!(trace.score <= NEG_INF / 2, "{}", context());
                    assert!(trace.events.is_empty(), "{}", context());
                    continue;
                }
                assert_eq!(trace.score, expected, "{}", context());
                assert_eq!(rescore(&trace, &a, &b, &GAPS), trace.score, "{}", context());
                assert_well_formed(&trace, a.len(), b.len(), mode, Some(&mask));
            }
        }
    }

    #[test]
    fn a_masked_global_border_is_a_wall() {
        // Skipping a masked row is impossible, so with row 0 masked a global
        // alignment can't start at all.
        let a = parse("K AE1 T");
        let mut mask = Mask::new(a.len(), a.len());
        mask.rows[0] = true;
        let trace = run(&a, &a, Mode::Global, Some(&mask), &GAPS);
        assert!(trace.score <= NEG_INF / 2);
        assert!(trace.events.is_empty());
    }

    #[test]
    fn ties_prefer_the_documented_cells_and_states() {
        // Semi-global: two equal ends, (1, 1) and (1, 2): (n, m) wins.
        let trace = run(
            &parse("AE1"),
            &parse("AE1 AE1"),
            Mode::SemiGlobal,
            None,
            &GAPS,
        );
        assert_eq!(trace.events, [TraceEvent::Pair { i: 0, j: 1 }]);
        // Local: two equal best regions; the first in row-major order wins.
        let (a, b) = (parse("K AE1 T"), parse("K AE1 T K AE1 T"));
        let trace = run(&a, &b, Mode::Local, None, &GAPS);
        assert_eq!(
            trace.events,
            [
                TraceEvent::Pair { i: 0, j: 0 },
                TraceEvent::Pair { i: 1, j: 1 },
                TraceEvent::Pair { i: 2, j: 2 },
            ]
        );
        // Global, free gaps: skipping both beats pairing K with T, and the
        // end state prefers X (ending on a skipped `a`) over Y.
        let free = GapModel {
            open: 0,
            open_repeated_consonant: 0,
            extend_vowel: 0,
            extend_consonant: 0,
            extend_repeated_consonant: 0,
        };
        let trace = run(&parse("K"), &parse("T"), Mode::Global, None, &free);
        assert_eq!(trace.score, 0);
        assert_eq!(
            trace.events,
            [TraceEvent::SkipB { j: 0 }, TraceEvent::SkipA { i: 0 }]
        );
    }

    #[test]
    fn traced_and_untraced_scores_agree_on_longer_inputs() {
        let alphabet = parse("AE1 AE0 IH0 OW1 ER0 K T T S M N D");
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        for _ in 0..300 {
            let a = rng.sequence(&alphabet, 14);
            let b = rng.sequence(&alphabet, 14);
            for mode in modes() {
                let traced = run(&a, &b, mode, None, &GAPS);
                let untraced = align(
                    &phoneme_ids(&a),
                    &phoneme_ids(&b),
                    &table(),
                    &GapScores::new(&GAPS, &a, None),
                    &GapScores::new(&GAPS, &b, None),
                    mode,
                    None,
                    false,
                );
                assert_eq!(traced.score, untraced.score);
                assert_eq!(rescore(&traced, &a, &b, &GAPS), traced.score);
                assert_well_formed(&traced, a.len(), b.len(), mode, None);
            }
        }
    }

    #[test]
    fn global_with_no_open_cost_equals_the_linear_recurrence() {
        let alphabet = parse("AE1 AE0 IH0 OW1 K T T S M");
        let gaps = GapModel {
            open: 0,
            open_repeated_consonant: 0,
            ..GAPS
        };
        let mut rng = Rng(0x5DEE_CE66_D1CE_4E5B);
        for _ in 0..1000 {
            let a = rng.sequence(&alphabet, 8);
            let b = rng.sequence(&alphabet, 8);
            let affine = run(&a, &b, Mode::Global, None, &gaps);
            let linear = linear::align_global(&a, &b, &table(), &gaps, None, None, None, false);
            assert_eq!(affine.score, linear.score, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn semi_global_skips_free_ends() {
        let (a, b) = (parse("D AA1 R K"), parse("DH AH0 D AA1 R K ER0"));
        let trace = run(&a, &b, Mode::SemiGlobal, None, &GAPS);
        assert_eq!(trace.score, 22 + 40 + 22 + 22);
        assert_eq!(trace.events.first(), Some(&TraceEvent::Pair { i: 0, j: 2 }));
    }

    #[test]
    fn local_finds_the_best_region() {
        let (a, b) = (parse("S S K AO1 L"), parse("M K AO1 L T"));
        let trace = run(&a, &b, Mode::Local, None, &GAPS);
        assert_eq!(trace.score, 22 + 40 + 22);
        assert_eq!(
            trace.events,
            [
                TraceEvent::Pair { i: 2, j: 1 },
                TraceEvent::Pair { i: 3, j: 2 },
                TraceEvent::Pair { i: 4, j: 3 },
            ]
        );
        // Nothing alike: an empty alignment scoring 0.
        let trace = run(&parse("S"), &parse("AE1"), Mode::Local, None, &GAPS);
        assert_eq!((trace.score, trace.events.len()), (0, 0));
    }
}
