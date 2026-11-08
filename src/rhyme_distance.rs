pub use crate::align::{AlignmentStep, OwnedAlignmentStep, TranspositionDirection};
use crate::align::{GapModel, ScoreTable, Trace, linear};
use crate::consonant_distance::{
    AnalyticConsonantDistance, AnalyticConsonantDistanceConfig, ConsonantDistanceScorer,
};
use crate::phoneme::{Phoneme, get_syllable_count};
use crate::vowel_distance::{VowelDistanceScorer, VowelHexGraph, VowelHexGraphConfig};

/// Costs for the rhyme-distance DP.
///
/// Both phoneme-distance engines return distances normalized to
/// `0..=`[`crate::NORMALIZED_DISTANCE_MAX`] (1000 = the engine's most distant
/// pair), so every value here is calibrated against that fixed scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhymeDistanceConfig {
    /// We never want to compare a vowel to a consonant, so we should give this an arbitrarily high penalty.
    pub vowel_to_consonant_mismatch: u32,
    /// Multiplier on normalized vowel distances (0-1000).
    ///
    /// Together with `consonant_coefficient` this determines "how much more
    /// important vowels are than consonants": the default 7:1 comes from
    /// experimentation (see web/tuning) — a much higher ratio than I would
    /// have guessed from the armchair. This is the value that seems to maximize our ability to differntiate the different segments of aural rhyme differences.
    pub vowel_coefficient: u32,
    /// Multiplier on normalized consonant distances (0-1000): how much
    /// consonant differences contribute to rhyme distance.
    pub consonant_coefficient: u32,
    /// Ideally, we should be comparing equal syllable slices, in which case, the insertion/deletion penalty for vowels should be arbitrarily high, as we always want the vowels to align.
    ///
    /// TODO: allow comparing different sized syllable slices? (in which case I think what you want to do is... find the smaller-sized slice of the larger one with the lowest distance score)
    pub vowel_indel_penalty: u32,
    /// Insertion/deletion penalty for consonants
    ///
    /// This should be greater or equal to half the cost of substituting maximally distant consonants (`1000 * consonant_coefficient`), otherwise it's cheaper to insert/delete a consonant than to compare unrelated consonants.
    ///
    /// TODO: Should all inserted consonants have the same insertion penalty? Are some consonants "lighter" than others? For example adding a /z/ (to form a plural) seems like it should be lighter than adding a /k/?.
    pub consonant_indel_penalty: u32,
    /// "Reapeated" consonants should be penalized less than standard INDEL penalty, for example when comparing TEN DRY <=> TEND DRY (bad example, you get the idea)
    ///
    /// TODO: an /R/ following /ER/ should be counted as repeated
    ///
    /// TODO: a /W/ following /OW/ should be counted as repeated
    ///
    /// TODO: are there other vowel -> consonant sequences that should count as repeated?
    ///
    /// TODO: This should take into account cases like RAISING (R EY1 Z IH0 NG) vs RAISE SING (R EY1 Z S IH1 NG), i.e. we should take consonant distance of repeated consonants into account
    ///
    ///
    /// An arbitrary starting point would probably be (consonant_indel_penalty + distance())/2
    pub consonant_repeated_penalty: u32,
    /// Transposing consonants should incur a smaller penalty than comparing them each in order. E.g. MOST <=> MOATS should have relatively small penalty.
    ///
    /// TODO: What should this value be?
    pub transposition_cost: u32,
}

impl Default for RhymeDistanceConfig {
    fn default() -> Self {
        Self {
            vowel_to_consonant_mismatch: 100_000,
            vowel_coefficient: 7,
            consonant_coefficient: 1,
            vowel_indel_penalty: 100_000,
            consonant_indel_penalty: 500,
            consonant_repeated_penalty: 100,
            transposition_cost: 200,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RhymeDistanceOutput<'a> {
    score: u32,
    alignment: Vec<AlignmentStep<'a>>,
}

impl<'a> RhymeDistanceOutput<'a> {
    pub fn score(&self) -> u32 {
        self.score
    }

    /// Aligned edit steps in display order.
    pub fn alignment(&self) -> &[AlignmentStep<'a>] {
        &self.alignment
    }
}

/// Scores rhyme distance on the shared alignment engine
/// ([`crate::align`]): a global, linear-gap alignment with adjacent
/// consonant transpositions, in the `score = −cost` profile.
///
/// The phoneme engines are consulted once per configuration, to fill a
/// [`ScoreTable`] of negated pair costs; every setter rebuilds it.
#[derive(Clone)]
pub struct RhymeDistanceScorer<V = VowelHexGraph, C = AnalyticConsonantDistance>
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    vowel_scorer: V,
    consonant_scorer: C,
    config: RhymeDistanceConfig,
    /// `−(engine distance × coefficient)` for same-class pairs,
    /// `−vowel_to_consonant_mismatch` across classes.
    table: ScoreTable,
    /// Open 0; extend = the per-phoneme indel penalties.
    gaps: GapModel,
}

/// The rhyme profile's pair scores: negated costs.
fn rhyme_table<V, C>(vowels: &V, consonants: &C, config: &RhymeDistanceConfig) -> ScoreTable
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    ScoreTable::from_fn(|p, q| {
        let cost = match (p, q) {
            (Phoneme::Vowel(v1), Phoneme::Vowel(v2)) => {
                i64::from(vowels.distance(v1, v2)) * i64::from(config.vowel_coefficient)
            }
            (Phoneme::Consonant(c1), Phoneme::Consonant(c2)) => {
                i64::from(consonants.distance(c1, c2)) * i64::from(config.consonant_coefficient)
            }
            _ => i64::from(config.vowel_to_consonant_mismatch),
        };
        -cost
    })
}

/// The rhyme profile's gaps: linear, each gapped phoneme paying its indel
/// penalty.
fn rhyme_gaps(config: &RhymeDistanceConfig) -> GapModel {
    GapModel {
        open: 0,
        open_repeated_consonant: 0,
        extend_vowel: i64::from(config.vowel_indel_penalty),
        extend_consonant: i64::from(config.consonant_indel_penalty),
        extend_repeated_consonant: i64::from(config.consonant_repeated_penalty),
    }
}

/// A non-negative engine cost as a rhyme distance, saturating at the limit.
fn cost_to_u32(cost: i64) -> u32 {
    u32::try_from(cost).unwrap_or(u32::MAX)
}

impl RhymeDistanceScorer<VowelHexGraph, AnalyticConsonantDistance> {
    pub fn new() -> Self {
        Self::with_config(RhymeDistanceConfig::default())
    }

    pub fn with_config(config: RhymeDistanceConfig) -> Self {
        Self::with_scorers_and_config(
            VowelHexGraph::new(),
            AnalyticConsonantDistance::new(),
            config,
        )
    }

    pub fn vowel_config(&self) -> VowelHexGraphConfig {
        self.vowel_scorer.config()
    }

    pub fn set_vowel_config(&mut self, config: VowelHexGraphConfig) {
        self.vowel_scorer = VowelHexGraph::with_config(config);
        self.rebuild();
    }

    pub fn consonant_config(&self) -> AnalyticConsonantDistanceConfig {
        self.consonant_scorer.config()
    }

    pub fn set_consonant_config(&mut self, config: AnalyticConsonantDistanceConfig) {
        self.consonant_scorer = AnalyticConsonantDistance::with_config(config);
        self.rebuild();
    }
}

impl<V> RhymeDistanceScorer<V, AnalyticConsonantDistance>
where
    V: VowelDistanceScorer,
{
    pub fn with_vowel_scorer(vowel_scorer: V) -> Self {
        Self::with_scorers(vowel_scorer, AnalyticConsonantDistance::new())
    }
}

impl<C> RhymeDistanceScorer<VowelHexGraph, C>
where
    C: ConsonantDistanceScorer,
{
    pub fn with_consonant_scorer(consonant_scorer: C) -> Self {
        Self::with_scorers(VowelHexGraph::new(), consonant_scorer)
    }
}

impl<V, C> RhymeDistanceScorer<V, C>
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    pub fn with_scorers(vowel_scorer: V, consonant_scorer: C) -> Self {
        Self::with_scorers_and_config(
            vowel_scorer,
            consonant_scorer,
            RhymeDistanceConfig::default(),
        )
    }

    pub fn with_scorers_and_config(
        vowel_scorer: V,
        consonant_scorer: C,
        config: RhymeDistanceConfig,
    ) -> Self {
        let table = rhyme_table(&vowel_scorer, &consonant_scorer, &config);
        Self {
            vowel_scorer,
            consonant_scorer,
            config,
            table,
            gaps: rhyme_gaps(&config),
        }
    }

    pub fn config(&self) -> RhymeDistanceConfig {
        self.config
    }

    pub fn set_config(&mut self, config: RhymeDistanceConfig) {
        self.config = config;
        self.rebuild();
    }

    /// Re-derives the score table and gap model from the engines and config.
    /// Every setter must call this, or scoring keeps the old costs.
    fn rebuild(&mut self) {
        self.table = rhyme_table(&self.vowel_scorer, &self.consonant_scorer, &self.config);
        self.gaps = rhyme_gaps(&self.config);
    }

    fn gap_penalty(&self, phoneme: &Phoneme, prev: Option<&Phoneme>) -> u32 {
        cost_to_u32(self.gaps.open_cost(phoneme, prev) + self.gaps.extend_cost(phoneme, prev))
    }

    fn substitution_score(&self, p1: &Phoneme, p2: &Phoneme) -> u32 {
        cost_to_u32(-self.table.score(p1, p2))
    }

    /// Runs the engine's linear global recurrence on `s1` vs `s2`, with
    /// `prefix_context` as the gap context before each first phoneme.
    fn run(
        &self,
        s1: &[Phoneme],
        s2: &[Phoneme],
        prefix_context: Option<&Phoneme>,
        trace: bool,
    ) -> Trace {
        linear::align_global(
            s1,
            s2,
            &self.table,
            &self.gaps,
            prefix_context,
            prefix_context,
            Some(i64::from(self.config.transposition_cost)),
            trace,
        )
    }

    /// The length of the common prefix of `slice1` and `slice2`, and the
    /// phoneme just before the end of it (the gap context it leaves behind).
    fn common_prefix<'p>(
        slice1: &'p [Phoneme],
        slice2: &[Phoneme],
    ) -> (usize, Option<&'p Phoneme>) {
        let prefix = slice1
            .iter()
            .zip(slice2.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let prefix_context = prefix.checked_sub(1).map(|last| &slice1[last]);
        (prefix, prefix_context)
    }

    pub fn align<'a>(
        &self,
        slice1: &'a [Phoneme],
        slice2: &'a [Phoneme],
    ) -> RhymeDistanceOutput<'a> {
        // Strip the common prefix before running the DP.
        //
        // Why prefix stripping is safe (and suffix stripping is NOT):
        // - No transposition can straddle the boundary: a swap crossing it
        //   would require the two phonemes at the first mismatch to be equal,
        //   contradicting that the strip stopped there.
        // - Gap costs look only backward (the repeated-consonant discount keys
        //   on the previous phoneme in the same sequence), and that context is
        //   preserved across the boundary via `prefix_context`.
        // - Matching a run of equal phonemes at its *earliest* occurrence
        //   keeps every discount available: leftover duplicates are gapped
        //   with their equal predecessor as context. Suffix stripping pins the
        //   *latest* occurrence instead, forcing an earlier duplicate to be
        //   gapped at full price. Counterexample that used to overcharge:
        //   "IH0 B" vs "P T B B" — the cheap insertion of the final B (prev B
        //   => repeated discount) is only reachable when the DP may match B
        //   with the first B of the pair. `dp_matches_exhaustive_oracle`
        //   guards this.
        let (prefix, prefix_context) = Self::common_prefix(slice1, slice2);
        let s1 = &slice1[prefix..];
        let s2 = &slice2[prefix..];

        let trace = self.run(s1, s2, prefix_context, true);
        let mut alignment: Vec<AlignmentStep<'a>> = slice1[..prefix]
            .iter()
            .zip(&slice2[..prefix])
            .map(|(a, b)| AlignmentStep::Match { a, b })
            .collect();
        alignment.extend(trace.steps(s1, s2));

        RhymeDistanceOutput {
            score: cost_to_u32(-trace.score),
            alignment,
        }
    }

    pub fn distance(&self, slice1: &[Phoneme], slice2: &[Phoneme]) -> u32 {
        // Strip common prefix. Only the prefix: see `align` for why prefix
        // stripping preserves optimality while suffix stripping would not.
        let (prefix, prefix_context) = Self::common_prefix(slice1, slice2);
        let s1 = &slice1[prefix..];
        let s2 = &slice2[prefix..];

        // Ensure s1 is the shorter slice. Safe because the cost model is
        // symmetric: gap contexts are same-sequence, substitution and
        // transposition costs don't depend on orientation.
        let (s1, s2) = if s2.len() < s1.len() {
            (s2, s1)
        } else {
            (s1, s2)
        };

        cost_to_u32(-self.run(s1, s2, prefix_context, false).score)
    }

    /// The per-step cost of an alignment, parallel to `steps`, such that the
    /// costs sum to the alignment's score. This is on-demand introspection
    /// (e.g. showing which steps drive a rhyme distance) — it isn't needed to
    /// score or align, so it's kept out of the alignment itself.
    ///
    /// Must be called with the same scorer that produced the alignment;
    /// otherwise the costs won't correspond to it.
    pub fn step_costs(&self, steps: &[OwnedAlignmentStep]) -> Vec<u32> {
        let mut costs = Vec::with_capacity(steps.len());
        // The previous phoneme in each sequence, for the repeated-consonant gap
        // discount. An s1 phoneme is always an `a` side, s2 always a `b` side,
        // so tracking the last of each in alignment order gives the right prev.
        let mut prev_a: Option<&Phoneme> = None;
        let mut prev_b: Option<&Phoneme> = None;

        for step in steps {
            let cost = match step {
                OwnedAlignmentStep::Match { a, b } | OwnedAlignmentStep::Substitution { a, b } => {
                    self.substitution_score(a, b)
                }
                OwnedAlignmentStep::Indel {
                    a: Some(a),
                    b: None,
                } => self.gap_penalty(a, prev_a),
                OwnedAlignmentStep::Indel {
                    a: None,
                    b: Some(b),
                } => self.gap_penalty(b, prev_b),
                OwnedAlignmentStep::Indel { .. } => unreachable!("an indel has exactly one side"),
                // Transposed endpoints are matched diagonally at cost 0; the
                // fixed transposition_cost is charged once, on the closing
                // (Left) step of the pair.
                OwnedAlignmentStep::Transposition {
                    direction: TranspositionDirection::Left,
                    ..
                } => self.config.transposition_cost,
                OwnedAlignmentStep::Transposition { .. } => 0,
            };
            costs.push(cost);

            if let Some(a) = step.a() {
                prev_a = Some(a);
            }
            if let Some(b) = step.b() {
                prev_b = Some(b);
            }
        }
        costs
    }
}

impl Default for RhymeDistanceScorer<VowelHexGraph, AnalyticConsonantDistance> {
    fn default() -> Self {
        Self::new()
    }
}

/// How a raw rhyme distance is scaled so that scores are comparable across
/// rhyming parts of different lengths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RhymeScoreNormalization {
    None,
    PerSyllable,
    #[default]
    PerMeanPhoneme,
}

impl RhymeScoreNormalization {
    pub fn denominator(
        self,
        syllable_count: usize,
        leader_phoneme_count: usize,
        target_phoneme_count: usize,
    ) -> f32 {
        match self {
            Self::None => 1.0,
            Self::PerSyllable => syllable_count as f32,
            Self::PerMeanPhoneme => (leader_phoneme_count + target_phoneme_count) as f32 / 2.0,
        }
    }
}

/// The measurement half of a rhyme comparison: the raw DP distance of one
/// (leader part, target part) pair, plus its normalization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RhymePairScore {
    pub raw_distance: u32,
    pub target_syllable_count: usize,
    pub leader_len: usize,
    pub target_len: usize,
}

impl RhymePairScore {
    pub fn normalized(&self, n: RhymeScoreNormalization) -> f32 {
        self.raw_distance as f32
            / n.denominator(self.target_syllable_count, self.leader_len, self.target_len)
    }
}

/// Scores one pair of phoneme slices. This is the single definition of
/// "normalized rhyme distance": both the poem checker
/// (`rhyme_scheme::best_rhyme_distance`) and dictionary-wide scans
/// (`Analyzer::find_rhymes`) go through it.
pub(crate) fn score_rhyme_pair<V, C>(
    scorer: &RhymeDistanceScorer<V, C>,
    leader_part: &[Phoneme],
    target_part: &[Phoneme],
) -> RhymePairScore
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    let target_syllable_count = get_syllable_count(target_part);
    let raw_distance = scorer.distance(leader_part, target_part);
    RhymePairScore {
        raw_distance,
        target_syllable_count,
        leader_len: leader_part.len(),
        target_len: target_part.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Consonant, Vowel, consonant_distance::ConsonantDistanceScorer,
        vowel_distance::VowelDistanceScorer,
    };
    use std::sync::OnceLock;

    const INF: u32 = u32::MAX / 2;

    struct FixedVowelScorer;

    impl VowelDistanceScorer for FixedVowelScorer {
        fn distance(&self, v1: &Vowel, v2: &Vowel) -> u32 {
            if v1 == v2 { 0 } else { 7 }
        }
    }

    struct FixedConsonantScorer;

    impl ConsonantDistanceScorer for FixedConsonantScorer {
        fn distance(&self, c1: &Consonant, c2: &Consonant) -> u32 {
            if c1 == c2 { 0 } else { 3 }
        }
    }

    static SCORER: OnceLock<RhymeDistanceScorer> = OnceLock::new();
    fn scorer() -> &'static RhymeDistanceScorer {
        SCORER.get_or_init(RhymeDistanceScorer::new)
    }

    fn config() -> RhymeDistanceConfig {
        RhymeDistanceConfig::default()
    }

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    fn vowel(s: &str) -> Vowel {
        s.parse().unwrap()
    }

    fn consonant(s: &str) -> Consonant {
        match s.parse::<Phoneme>().unwrap() {
            Phoneme::Consonant(c) => c,
            other => panic!("not a consonant: {other:?}"),
        }
    }

    /// Recomputes the cost of an `align` result from its own steps, using the
    /// same cost primitives as the DP, and asserts it equals `score()`. This is
    /// the invariant that a forward/traceback cost mismatch violates.
    fn assert_score_matches_alignment(s: &RhymeDistanceScorer, s1: &[Phoneme], s2: &[Phoneme]) {
        let output = s.align(s1, s2);
        assert_transpositions_adjacent(output.alignment());

        // Independent per-step cost derivation, kept separate from the
        // production `step_costs` so it can cross-check it.
        let mut reference = Vec::with_capacity(output.alignment().len());
        let mut prev_a: Option<&Phoneme> = None;
        let mut prev_b: Option<&Phoneme> = None;
        for step in output.alignment() {
            // AlignmentStep is Copy, so matching by value binds &Phoneme (not &&).
            let cost = match *step {
                AlignmentStep::Match { a, b } | AlignmentStep::Substitution { a, b } => {
                    s.substitution_score(a, b)
                }
                AlignmentStep::Indel {
                    a: Some(a),
                    b: None,
                } => s.gap_penalty(a, prev_a),
                AlignmentStep::Indel {
                    a: None,
                    b: Some(b),
                } => s.gap_penalty(b, prev_b),
                AlignmentStep::Indel { .. } => unreachable!("an indel has exactly one side"),
                AlignmentStep::Transposition { direction, .. } => {
                    // The step's `a`/`b` are the column phonemes (which differ),
                    // but the DP matches them diagonally across the swap at cost
                    // 0 — a transposition only fires when the swapped phonemes
                    // are equal. So the endpoints add nothing; the fixed
                    // transposition_cost is counted once, on the closing (Left)
                    // step of the pair.
                    if direction == TranspositionDirection::Left {
                        s.config().transposition_cost
                    } else {
                        0
                    }
                }
            };
            reference.push(cost);

            match *step {
                AlignmentStep::Match { a, b }
                | AlignmentStep::Substitution { a, b }
                | AlignmentStep::Transposition { a, b, .. } => {
                    prev_a = Some(a);
                    prev_b = Some(b);
                }
                AlignmentStep::Indel { a, b } => {
                    if let Some(a) = a {
                        prev_a = Some(a);
                    }
                    if let Some(b) = b {
                        prev_b = Some(b);
                    }
                }
            }
        }

        // 1. The independent derivation must sum to the DP's score — this is the
        //    forward/traceback consistency guard.
        assert_eq!(
            reference.iter().sum::<u32>(),
            output.score,
            "summed step costs must equal align().score()\n  s1 = {s1:?}\n  s2 = {s2:?}\n  alignment = {:?}",
            output.alignment(),
        );

        // 2. Production `step_costs` must reproduce it exactly, per step.
        let owned: Vec<OwnedAlignmentStep> = output
            .alignment()
            .iter()
            .map(|s| OwnedAlignmentStep::from(*s))
            .collect();
        assert_eq!(
            s.step_costs(&owned),
            reference,
            "step_costs must match the independent derivation for {s1:?} vs {s2:?}",
        );
    }

    #[test]
    fn score_equals_summed_alignment_cost() {
        let s = scorer();

        // Curated cases covering every step kind.
        for (a, b) in [
            ("", ""),
            ("AE1 T", "AE1 T"),                 // all matches
            ("AE1 T", "AA1 T"),                 // vowel substitution
            ("B T", "B IH1 T"),                 // interior vowel indel
            ("M OW1 S T", "M OW1 T S"),         // transposition
            ("T EH1 N", "T EH1 N D"),           // repeated-consonant / trailing indel
            ("UW1 P R IH2 N T", "UW1 B AE1 G"), // the blueprint/new bag case
        ] {
            assert_score_matches_alignment(s, &parse(a), &parse(b));
        }

        // Deterministic pseudo-random sweep (LCG, fixed seed — no rand dep and
        // reproducible) over a small alphabet with repeats and mixed vowels/
        // consonants, to exercise transpositions and the repeated-consonant
        // discount across many shapes.
        let alphabet = parse("AE1 IH0 UW1 OW1 K T S P B M");
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rng = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };
        for _ in 0..2000 {
            let len1 = 1 + rng() % 6;
            let len2 = 1 + rng() % 6;
            let s1: Vec<Phoneme> = (0..len1)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            let s2: Vec<Phoneme> = (0..len2)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            assert_score_matches_alignment(s, &s1, &s2);
        }
    }

    /// Ground-truth distance by exhaustive recursion over every edit sequence
    /// (delete / insert / substitute / adjacent consonant transposition),
    /// sharing the cost primitives but none of the machinery under test: no
    /// matrix, no boundary initialization, no prefix/suffix stripping, no
    /// slice swapping.
    /// This is an *implementation* oracle — it validates that the DP and its
    /// wrappers faithfully minimize the OSA recurrence, not that OSA is the
    /// right model. Exponential, so only for tiny inputs.
    fn oracle_distance(
        s: &RhymeDistanceScorer,
        s1: &[Phoneme],
        s2: &[Phoneme],
        i: usize,
        j: usize,
    ) -> u32 {
        if i == s1.len() && j == s2.len() {
            return 0;
        }

        let mut best = INF;

        // Delete s1[i].
        if i < s1.len() {
            let prev = if i > 0 { Some(&s1[i - 1]) } else { None };
            let cost = s.gap_penalty(&s1[i], prev);
            best = best.min(cost.saturating_add(oracle_distance(s, s1, s2, i + 1, j)));
        }
        // Insert s2[j].
        if j < s2.len() {
            let prev = if j > 0 { Some(&s2[j - 1]) } else { None };
            let cost = s.gap_penalty(&s2[j], prev);
            best = best.min(cost.saturating_add(oracle_distance(s, s1, s2, i, j + 1)));
        }
        // Substitute (or match) s1[i] with s2[j].
        if i < s1.len() && j < s2.len() {
            let cost = s.substitution_score(&s1[i], &s2[j]);
            best = best.min(cost.saturating_add(oracle_distance(s, s1, s2, i + 1, j + 1)));
        }
        // Adjacent consonant transposition: s1[i],s1[i+1] crossed with
        // s2[j],s2[j+1].
        if i + 1 < s1.len()
            && j + 1 < s2.len()
            && matches!(
                (&s1[i], &s1[i + 1]),
                (Phoneme::Consonant(_), Phoneme::Consonant(_))
            )
            && s1[i] == s2[j + 1]
            && s1[i + 1] == s2[j]
        {
            let cost = s.config().transposition_cost;
            best = best.min(cost.saturating_add(oracle_distance(s, s1, s2, i + 2, j + 2)));
        }

        best
    }

    #[test]
    fn dp_matches_exhaustive_oracle() {
        let s = scorer();

        // Curated shapes: empties, pure matches, substitutions, interior gaps,
        // transpositions, repeated consonants, and the historical regressions.
        let curated = [
            ("", ""),
            ("B", ""),
            ("", "AE1"),
            ("AE1 T", "AE1 T"),
            ("AE1 T", "AA1 T"),
            ("B T", "B IH1 T"),
            ("M OW1 S T", "M OW1 T S"),
            ("AE1 IH0", "IH0 AE1"),
            ("AE1 T", "T AE1"),
            ("B B", "B"),
            ("T EH1 N", "T EH1 N D"),
            ("M P S", "S P M"),
        ];
        for (a, b) in curated {
            let (pa, pb) = (parse(a), parse(b));
            assert_eq!(
                s.distance(&pa, &pb),
                oracle_distance(s, &pa, &pb, 0, 0),
                "distance() diverged from the exhaustive oracle for {a:?} vs {b:?}"
            );
        }

        // Deterministic pseudo-random sweep. Lengths stay tiny because the
        // oracle is exponential; the alphabet mixes vowels and consonants with
        // repeats so gaps, discounts, and transpositions all occur.
        let alphabet = parse("AE1 IH0 UW1 OW1 K T S P B M");
        let mut state: u64 = 0xD1B5_4A32_D192_ED03;
        let mut rng = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };
        for _ in 0..500 {
            let len1 = rng() % 5;
            let len2 = rng() % 5;
            let s1: Vec<Phoneme> = (0..len1)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();
            let s2: Vec<Phoneme> = (0..len2)
                .map(|_| alphabet[rng() % alphabet.len()].clone())
                .collect();

            let expected = oracle_distance(s, &s1, &s2, 0, 0);
            assert_eq!(
                s.distance(&s1, &s2),
                expected,
                "distance() diverged from the exhaustive oracle for {s1:?} vs {s2:?}"
            );
            // Symmetry and align/distance agreement ride along on the same corpus.
            assert_eq!(
                s.distance(&s2, &s1),
                expected,
                "distance() must be symmetric for {s1:?} vs {s2:?}"
            );
            assert_eq!(
                s.align(&s1, &s2).score,
                expected,
                "align() score diverged from the exhaustive oracle for {s1:?} vs {s2:?}"
            );
        }
    }

    #[test]
    fn interior_gap_charges_the_gapped_phoneme() {
        // Regression: the DP once swapped the insert/delete gap costs, charging
        // an interior gap by the *other* sequence's current phoneme. That let a
        // vowel be indel'd for the price of an adjacent consonant, dodging the
        // vowel_indel_penalty (e.g. "blueprint" vs "new bag" scored far too low).
        //
        // Matching consonants on both sides force the vowel to be an interior
        // gap. Both orientations are checked: inserting the vowel skips a `b`
        // phoneme, deleting it skips an `a` phoneme.
        let s = scorer();
        let vowel_indel = config().vowel_indel_penalty;
        assert_eq!(
            s.align(&parse("B T"), &parse("B IH1 T")).score(),
            vowel_indel
        );
        assert_eq!(
            s.align(&parse("B IH1 T"), &parse("B T")).score(),
            vowel_indel
        );
        // A consonant gap is still cheap — the two penalties aren't collapsed.
        assert_eq!(
            s.align(&parse("B T"), &parse("B M T")).score(),
            config().consonant_indel_penalty
        );
    }

    fn aligned_tokens(output: &RhymeDistanceOutput<'_>) -> (Vec<String>, Vec<String>) {
        output
            .alignment()
            .iter()
            .map(|step| match step {
                AlignmentStep::Match { a, b } | AlignmentStep::Substitution { a, b } => {
                    (a.to_string(), b.to_string())
                }
                AlignmentStep::Indel { a, b } => (
                    a.map(ToString::to_string).unwrap_or("_".to_string()),
                    b.map(ToString::to_string).unwrap_or("_".to_string()),
                ),
                AlignmentStep::Transposition { a, b, direction } => {
                    let a_marker = direction.as_char();
                    let b_marker = match direction {
                        TranspositionDirection::Right => '/',
                        TranspositionDirection::Left => '\\',
                    };
                    (format!("{a}{a_marker}"), format!("{b}{b_marker}"))
                }
            })
            .unzip()
    }

    #[test]
    fn equal_slices_are_zero() {
        let s = parse("M AO1 S T");
        assert_eq!(scorer().distance(&s, &s), 0);
    }

    #[test]
    fn empty_vs_consonants() {
        let s = parse("M AO1 S T");
        // Aligning against nothing gaps every phoneme: three consonant
        // indels plus one (much pricier) vowel indel.
        assert_eq!(
            scorer().distance(&[], &s),
            config().consonant_indel_penalty * 3 + config().vowel_indel_penalty
        );
        assert_eq!(
            scorer().distance(&s, &[]),
            config().consonant_indel_penalty * 3 + config().vowel_indel_penalty
        );
    }

    #[test]
    fn both_empty() {
        assert_eq!(scorer().distance(&[], &[]), 0);
    }

    #[test]
    fn adjacent_vowel_substitution() {
        // AO1 vs OW1: adjacent on the hex-graph, same stress. The DP charges
        // the engine's normalized distance times the vowel coefficient.
        let s1 = parse("M AO1 S T");
        let s2 = parse("M OW1 S T");
        assert_eq!(
            scorer().distance(&s1, &s2),
            VowelHexGraph::new().distance(&vowel("AO1"), &vowel("OW1"))
                * config().vowel_coefficient
        );
    }

    #[test]
    fn with_vowel_scorer_uses_custom_vowel_distance() {
        let scorer = RhymeDistanceScorer::with_vowel_scorer(FixedVowelScorer);
        let s1 = parse("AO1");
        let s2 = parse("OW1");

        assert_eq!(scorer.distance(&s1, &s2), 7 * config().vowel_coefficient);
    }

    #[test]
    fn with_consonant_scorer_uses_custom_consonant_distance() {
        let scorer = RhymeDistanceScorer::with_consonant_scorer(FixedConsonantScorer);
        let s1 = parse("B");
        let s2 = parse("K");

        assert_eq!(scorer.distance(&s1, &s2), 3);
    }

    #[test]
    fn with_config_controls_edit_costs() {
        // Values sized against the engines' normalized 0-1000 scale, so that
        // each assertion's intended edit stays the cheapest option.
        let config = RhymeDistanceConfig {
            vowel_to_consonant_mismatch: 500,
            vowel_coefficient: 4,
            consonant_coefficient: 4,
            vowel_indel_penalty: 4000,
            consonant_indel_penalty: 600,
            consonant_repeated_penalty: 200,
            transposition_cost: 300,
        };
        let scorer = RhymeDistanceScorer::with_config(config);

        assert_eq!(
            scorer.distance(&parse("AO1"), &parse("OW1")),
            VowelHexGraph::new().distance(&vowel("AO1"), &vowel("OW1")) * config.vowel_coefficient
        );
        assert_eq!(scorer.distance(&parse("AO1"), &parse("B")), 500);
        assert_eq!(
            scorer.distance(&parse("T"), &parse("D")),
            AnalyticConsonantDistance::new().distance(&consonant("T"), &consonant("D"))
                * config.consonant_coefficient
        );
        assert_eq!(scorer.distance(&parse("B"), &[]), 600);
        assert_eq!(scorer.distance(&parse("B B"), &parse("B")), 200);
        assert_eq!(
            scorer.distance(&parse("M OW1 S T"), &parse("M OW1 T S")),
            300
        );
    }

    #[test]
    fn set_config_updates_existing_scorer() {
        let mut scorer = RhymeDistanceScorer::new();
        let mut config = scorer.config();
        config.consonant_coefficient = 4;

        scorer.set_config(config);

        assert_eq!(scorer.config(), config);
        assert_eq!(
            scorer.distance(&parse("T"), &parse("D")),
            AnalyticConsonantDistance::new().distance(&consonant("T"), &consonant("D"))
                * config.consonant_coefficient
        );
    }

    #[test]
    fn component_configs_update_without_resetting_rhyme_config() {
        let mut scorer = RhymeDistanceScorer::new();
        let rhyme_config = scorer.config();

        let mut vowel_config = scorer.vowel_config();
        vowel_config.vowel_distance_coefficient = 9;
        scorer.set_vowel_config(vowel_config);

        let mut consonant_config = scorer.consonant_config();
        consonant_config.voiced_penalty = 7;
        scorer.set_consonant_config(consonant_config);

        assert_eq!(scorer.config(), rhyme_config);
        assert_eq!(scorer.vowel_config(), vowel_config);
        assert_eq!(scorer.consonant_config(), consonant_config);
        // The updated component configs must actually drive scoring: costs
        // match engines freshly built with the same configs.
        assert_eq!(
            scorer.distance(&parse("AO1"), &parse("OW1")),
            VowelHexGraph::with_config(vowel_config).distance(&vowel("AO1"), &vowel("OW1"))
                * rhyme_config.vowel_coefficient
        );
        assert_eq!(
            scorer.distance(&parse("T"), &parse("D")),
            AnalyticConsonantDistance::with_config(consonant_config)
                .distance(&consonant("T"), &consonant("D"))
                * rhyme_config.consonant_coefficient
        );
    }

    #[test]
    fn consonant_substitution_voiced_pair() {
        // T vs D: same manner (Plosive), same place, differ only in voicing.
        // The DP charges the engine's normalized distance times the
        // consonant coefficient.
        let s1 = parse("T AE1 N");
        let s2 = parse("D AE1 N");
        assert_eq!(
            scorer().distance(&s1, &s2),
            AnalyticConsonantDistance::new().distance(&consonant("T"), &consonant("D"))
                * config().consonant_coefficient
        );
    }

    /// Every transposition must swap immediately-adjacent phonemes, which in
    /// this alignment representation means its `Right` marker is directly
    /// followed by its `Left` marker — no steps in between.
    fn assert_transpositions_adjacent(alignment: &[AlignmentStep<'_>]) {
        for (k, step) in alignment.iter().enumerate() {
            if matches!(
                step,
                AlignmentStep::Transposition {
                    direction: TranspositionDirection::Right,
                    ..
                }
            ) {
                assert!(
                    matches!(
                        alignment.get(k + 1),
                        Some(AlignmentStep::Transposition {
                            direction: TranspositionDirection::Left,
                            ..
                        })
                    ),
                    "a transposition must swap adjacent phonemes (Right directly followed by Left): {alignment:?}",
                );
            }
        }
    }

    fn assert_no_transpositions(alignment: &[AlignmentStep<'_>]) {
        assert!(
            alignment
                .iter()
                .all(|step| !matches!(step, AlignmentStep::Transposition { .. })),
            "alignment must not contain a transposition: {alignment:?}",
        );
    }

    #[test]
    fn transpositions_are_only_adjacent() {
        // "M P S" vs "S P M": the old algorithm transposed the *non-adjacent* M
        // and S across the P (charging the P as a gap on both sides). Now the
        // only legal transposition is of adjacent phonemes — here the DP deletes
        // M, swaps the adjacent P,S ↔ S,P, and re-inserts M, but never crosses a
        // gap.
        let s = scorer();
        for (a, b) in [("M P S", "S P M"), ("S P T", "T P S"), ("M K S", "S K M")] {
            let (pa, pb) = (parse(a), parse(b));
            assert_transpositions_adjacent(s.align(&pa, &pb).alignment());
        }
    }

    #[test]
    fn transposition_of_adjacent_consonants() {
        // "M AO1 S T" vs "M AO1 T S": last two consonants transposed
        // The two elements between last occurrences of each in the other are empty,
        // so cost = TRANSPOSITION_COST
        let s1 = parse("M OW1 S T");
        let s2 = parse("M OW1 T S");
        assert_eq!(scorer().distance(&s1, &s2), config().transposition_cost);
    }

    #[test]
    fn vowels_cannot_transpose() {
        let s1 = parse("M AE1 AH0 T");
        let s2 = parse("M AH0 AE1 T");
        let output = scorer().align(&s1, &s2);

        assert!(output.score() > config().transposition_cost);
        assert_no_transpositions(output.alignment());
    }

    #[test]
    fn vowel_and_consonant_cannot_transpose() {
        let s1 = parse("M AE1 T");
        let s2 = parse("M T AE1");
        let output = scorer().align(&s1, &s2);

        assert!(output.score() > config().transposition_cost);
        assert_no_transpositions(output.alignment());
    }

    #[test]
    fn align_reports_score_and_alignment_for_insertion() {
        let s1 = parse("M AO1 T");
        let s2 = parse("M AO1 S T");
        let output = scorer().align(&s1, &s2);
        let (aligned_1, aligned_2) = aligned_tokens(&output);

        assert_eq!(output.score(), config().consonant_indel_penalty);
        assert_eq!(aligned_1, ["M", "AO1", "_", "T"]);
        assert_eq!(aligned_2, ["M", "AO1", "S", "T"]);
        assert!(matches!(output.alignment()[0], AlignmentStep::Match { .. }));
        assert!(matches!(output.alignment()[2], AlignmentStep::Indel { .. }));
    }

    #[test]
    fn align_marks_adjacent_transposition() {
        let s1 = parse("M AO1 S T");
        let s2 = parse("M AO1 T S");
        let output = scorer().align(&s1, &s2);
        let (aligned_1, aligned_2) = aligned_tokens(&output);

        assert_eq!(output.score(), config().transposition_cost);
        assert_eq!(aligned_1, ["M", "AO1", "S\\", "T/"]);
        assert_eq!(aligned_2, ["M", "AO1", "T/", "S\\"]);
        assert!(matches!(
            output.alignment()[2],
            AlignmentStep::Transposition {
                direction: TranspositionDirection::Right,
                ..
            }
        ));
        assert!(matches!(
            output.alignment()[3],
            AlignmentStep::Transposition {
                direction: TranspositionDirection::Left,
                ..
            }
        ));
    }

    #[test]
    fn align_score_matches_distance() {
        let s1 = parse("M AO1 S T");
        let s2 = parse("B OW1 T");

        assert_eq!(
            scorer().align(&s1, &s2).score(),
            scorer().distance(&s1, &s2)
        );
        assert_eq!(
            scorer().align(&s2, &s1).score(),
            scorer().distance(&s2, &s1)
        );
    }

    #[test]
    fn single_consonant_insertion() {
        let s1 = parse("M AO1 T");
        let s2 = parse("M AO1 S T");
        assert_eq!(
            scorer().distance(&s1, &s2),
            config().consonant_indel_penalty
        );
    }

    #[test]
    fn repeated_consonant_penalty() {
        // Inserting a consonant identical to its neighbour costs CONSONANT_REPEATED_PENALTY
        let s1 = parse("B AE1 T IH0 NG");
        let s2 = parse("B AE1 T T IH0 NG");
        assert_eq!(
            scorer().distance(&s1, &s2),
            config().consonant_repeated_penalty
        );
    }

    #[test]
    fn distance_is_symmetric() {
        let s1 = parse("M AO1 S T");
        let s2 = parse("B OW1 T");
        assert_eq!(scorer().distance(&s1, &s2), scorer().distance(&s2, &s1));
    }

    #[test]
    fn normalization_denominators_match_their_units() {
        assert_eq!(RhymeScoreNormalization::None.denominator(2, 4, 5), 1.0);
        assert_eq!(
            RhymeScoreNormalization::PerSyllable.denominator(2, 4, 5),
            2.0
        );
        assert_eq!(
            RhymeScoreNormalization::PerMeanPhoneme.denominator(2, 4, 5),
            4.5
        );
    }
}
