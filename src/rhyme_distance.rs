use crate::consonant_distance::{AnalyticConsonantDistance, ConsonantDistanceScorer};
use crate::phoneme::Phoneme;
use crate::vowel_distance::{VowelDistanceScorer, VowelHexGraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhymeDistanceConfig {
    /// We never want to compare a vowel to a consonant, so we should give this an arbitrarily high penalty.
    pub vowel_to_consonant_mismatch: u32,
    /// Vowel distance ranges from 0-9 (VowelHexGraph distance (<= 4), gets multiplied by VOWEL_DISTANCE_COEFFICIENT (2), and VOWEL_STRESS_PENALTY (1) added), consonant distance range up to 10.
    ///
    /// This coefficient determines "how much more important vowels are than consonants"
    ///
    /// TODO find the correct value for this. My instinct is that vowels should be significantly more important than consonants.
    pub vowel_coefficient: u32,
    /// This coefficient determines how much consonant differences contribute to rhyme distance.
    pub consonant_coefficient: u32,
    /// Ideally, we should be comparing equal syllable slices, in which case, the insertion/deletion penalty for vowels should be arbitrarily high, as we always want the vowels to align.
    ///
    /// TODO: allow comparing different sized syllable slices? (in which case I think what you want to do is... find the smaller-sized slice of the larger one with the lowest distance score)
    pub vowel_indel_penalty: u32,
    /// Insertion/deletion penalty for consonants
    ///
    /// This should be greater or equal to half of the default unrelated consonant penalty, otherwise it's cheaper to insert/delete a consonant than to compare unrelated consonants.
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
    /// An arbitrary starting point would probably be (1 + distance())/2
    pub consonant_repeated_penalty: u32,
    /// Transposing consonants should incur a smaller penalty than comparing them each in order. E.g. MOST <=> MOATS should have relatively small penalty.
    ///
    /// TODO: What should this value be?
    pub transposition_cost: u32,
}

impl Default for RhymeDistanceConfig {
    fn default() -> Self {
        Self {
            vowel_to_consonant_mismatch: 1000,
            // A default value of 8 comes experimentation (see web/tuning). This is a much higher coefficient than I would have guessed from the armchair. It is the minimum value with which we can differentiate between distant slant rhymes and words that should not rhyme.
            vowel_coefficient: 8,
            consonant_coefficient: 1,
            vowel_indel_penalty: 1000,
            consonant_indel_penalty: 5,
            consonant_repeated_penalty: 1,
            transposition_cost: 2,
        }
    }
}

const INF: u32 = u32::MAX / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspositionDirection {
    /// This phoneme matches a later/right column in the other sequence.
    Right,
    /// This phoneme matches an earlier/left column in the other sequence.
    Left,
}

impl TranspositionDirection {
    pub fn as_char(self) -> char {
        match self {
            TranspositionDirection::Right => '\\',
            TranspositionDirection::Left => '/',
        }
    }
}

// Todo: should this be folded into Edit enum?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentStep<'a> {
    Match {
        a: &'a Phoneme,
        b: &'a Phoneme,
    },
    Substitution {
        a: &'a Phoneme,
        b: &'a Phoneme,
    },
    Indel {
        a: Option<&'a Phoneme>,
        b: Option<&'a Phoneme>,
    },
    Transposition {
        a: &'a Phoneme,
        b: &'a Phoneme,
        direction: TranspositionDirection,
    },
}

/// An owned [`AlignmentStep`], for keeping an alignment alive beyond the
/// borrowed slices it was computed from (e.g. inside cached analysis results).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedAlignmentStep {
    Match {
        a: Phoneme,
        b: Phoneme,
    },
    Substitution {
        a: Phoneme,
        b: Phoneme,
    },
    Indel {
        a: Option<Phoneme>,
        b: Option<Phoneme>,
    },
    Transposition {
        a: Phoneme,
        b: Phoneme,
        direction: TranspositionDirection,
    },
}

impl OwnedAlignmentStep {
    /// The first-sequence phoneme of this step (`None` for a gap).
    pub fn a(&self) -> Option<&Phoneme> {
        match self {
            OwnedAlignmentStep::Match { a, .. }
            | OwnedAlignmentStep::Substitution { a, .. }
            | OwnedAlignmentStep::Transposition { a, .. } => Some(a),
            OwnedAlignmentStep::Indel { a, .. } => a.as_ref(),
        }
    }

    /// The second-sequence phoneme of this step (`None` for a gap).
    pub fn b(&self) -> Option<&Phoneme> {
        match self {
            OwnedAlignmentStep::Match { b, .. }
            | OwnedAlignmentStep::Substitution { b, .. }
            | OwnedAlignmentStep::Transposition { b, .. } => Some(b),
            OwnedAlignmentStep::Indel { b, .. } => b.as_ref(),
        }
    }
}

impl From<AlignmentStep<'_>> for OwnedAlignmentStep {
    fn from(step: AlignmentStep<'_>) -> Self {
        match step {
            AlignmentStep::Match { a, b } => OwnedAlignmentStep::Match {
                a: a.clone(),
                b: b.clone(),
            },
            AlignmentStep::Substitution { a, b } => OwnedAlignmentStep::Substitution {
                a: a.clone(),
                b: b.clone(),
            },
            AlignmentStep::Indel { a, b } => OwnedAlignmentStep::Indel {
                a: a.cloned(),
                b: b.cloned(),
            },
            AlignmentStep::Transposition { a, b, direction } => OwnedAlignmentStep::Transposition {
                a: a.clone(),
                b: b.clone(),
                direction,
            },
        }
    }
}

// TODO: should this be folded into DpOutput?
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

pub struct RhymeDistanceScorer<V = VowelHexGraph, C = AnalyticConsonantDistance>
where
    V: VowelDistanceScorer,
    C: ConsonantDistanceScorer,
{
    vowel_scorer: V,
    consonant_scorer: C,
    config: RhymeDistanceConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Start,
    GapInFirst,
    GapInSecond,
    Substitute,
    Transpose,
}

struct DpOutput {
    score: u32,
    edits: Option<Vec<Vec<Edit>>>,
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
        Self {
            vowel_scorer,
            consonant_scorer,
            config,
        }
    }

    pub fn config(&self) -> RhymeDistanceConfig {
        self.config
    }

    pub fn set_config(&mut self, config: RhymeDistanceConfig) {
        self.config = config;
    }

    fn gap_penalty(&self, phoneme: &Phoneme, prev: Option<&Phoneme>) -> u32 {
        match phoneme {
            Phoneme::Vowel(_) => self.config.vowel_indel_penalty,
            Phoneme::Consonant(c) => {
                if let Some(Phoneme::Consonant(p)) = prev
                    && c.phoneme == p.phoneme
                {
                    return self.config.consonant_repeated_penalty;
                }

                self.config.consonant_indel_penalty
            }
        }
    }

    fn substitution_score(&self, p1: &Phoneme, p2: &Phoneme) -> u32 {
        match (p1, p2) {
            (Phoneme::Vowel(v1), Phoneme::Vowel(v2)) => {
                self.vowel_scorer.distance(v1, v2) * self.config.vowel_coefficient
            }
            (Phoneme::Consonant(c1), Phoneme::Consonant(c2)) => {
                self.consonant_scorer.distance(c1, c2) * self.config.consonant_coefficient
            }
            _ => self.config.vowel_to_consonant_mismatch,
        }
    }

    /// Picks the cheapest candidate; on equal cost the lower `priority` wins.
    /// Priorities (match 0 < transposition 1 < costly substitution 2 <
    /// insertion 3 < deletion 4) prefer the most "structural" explanation in
    /// the traceback and make it deterministic regardless of the order in
    /// which candidates are offered. Only the traceback is affected — every
    /// candidate has a distinct priority, so the chosen *cost* never depends
    /// on this ordering.
    fn choose_edit(best: &mut (u32, u8, Edit), cost: u32, priority: u8, edit: Edit) {
        if cost < best.0 || (cost == best.0 && priority < best.1) {
            *best = (cost, priority, edit);
        }
    }

    /// Weighted optimal-string-alignment (OSA) distance: substitutions,
    /// context-aware gaps, and adjacent transpositions.
    ///
    /// Matrix layout: `dists[i][j]` is the minimal cost of aligning `s1[..i]`
    /// with `s2[..j]`; the answer is `dists[len1][len2]`.
    ///
    /// OSA caveats (deliberate): unlike unrestricted Damerau-Levenshtein, no
    /// substring is edited more than once, and the result is not a metric (the
    /// triangle inequality can fail). Both are fine for rhyme scoring, where
    /// every comparison stands alone.
    fn run_dp(
        &self,
        s1: &[Phoneme],
        s2: &[Phoneme],
        prefix_context: Option<&Phoneme>,
        trace: bool,
    ) -> DpOutput {
        let len1 = s1.len();
        let len2 = s2.len();

        let mut dists = vec![vec![0u32; len2 + 1]; len1 + 1];
        let mut edits = trace.then(|| vec![vec![Edit::Start; len2 + 1]; len1 + 1]);

        // Boundaries: aligning a prefix of one sequence against nothing is the
        // running sum of its gap penalties. The gap context (for the repeated-
        // consonant discount) is the previous phoneme in the same sequence;
        // `prefix_context` stands in before element 0.
        for i in 1..=len1 {
            let prev = if i > 1 { Some(&s1[i - 2]) } else { prefix_context };
            dists[i][0] = dists[i - 1][0].saturating_add(self.gap_penalty(&s1[i - 1], prev));
            if let Some(edits) = edits.as_mut() {
                edits[i][0] = Edit::GapInSecond;
            }
        }
        for j in 1..=len2 {
            let prev = if j > 1 { Some(&s2[j - 2]) } else { prefix_context };
            dists[0][j] = dists[0][j - 1].saturating_add(self.gap_penalty(&s2[j - 1], prev));
            if let Some(edits) = edits.as_mut() {
                edits[0][j] = Edit::GapInFirst;
            }
        }

        for (i1, p1) in s1.iter().enumerate() {
            for (i2, p2) in s2.iter().enumerate() {
                // This iteration writes dists[i][j]: s1[..i] vs s2[..j], with
                // p1 = s1[i-1] and p2 = s2[j-1] the phonemes being consumed.
                let (i, j) = (i1 + 1, i2 + 1);

                let del_cost = self.gap_penalty(
                    p1,
                    if i1 > 0 {
                        s1.get(i1 - 1)
                    } else {
                        prefix_context
                    },
                );
                let ins_cost = self.gap_penalty(
                    p2,
                    if i2 > 0 {
                        s2.get(i2 - 1)
                    } else {
                        prefix_context
                    },
                );
                let sub_cost = self.substitution_score(p1, p2);

                let mut best = (INF, u8::MAX, Edit::Start);

                // Adjacent transposition (optimal string alignment): the pair
                // s1[i1-1],s1[i1] mirrors s2[i2-1],s2[i2]. Only immediately
                // adjacent swaps qualify — nothing may sit between the swapped
                // phonemes — so this steps back two cells in each sequence.
                if i1 > 0 && i2 > 0 && p1 == &s2[i2 - 1] && &s1[i1 - 1] == p2 {
                    let trans_cost =
                        dists[i - 2][j - 2].saturating_add(self.config.transposition_cost);
                    Self::choose_edit(&mut best, trans_cost, 1, Edit::Transpose);
                }

                Self::choose_edit(
                    &mut best,
                    dists[i - 1][j - 1].saturating_add(sub_cost),
                    if sub_cost == 0 { 0 } else { 2 },
                    Edit::Substitute,
                );
                // GapInFirst advances through s2 (inserts p2), so it costs
                // ins_cost; GapInSecond advances through s1 (deletes p1), so it
                // costs del_cost.
                Self::choose_edit(
                    &mut best,
                    dists[i][j - 1].saturating_add(ins_cost),
                    3,
                    Edit::GapInFirst,
                );
                Self::choose_edit(
                    &mut best,
                    dists[i - 1][j].saturating_add(del_cost),
                    4,
                    Edit::GapInSecond,
                );

                dists[i][j] = best.0;
                if let Some(edits) = edits.as_mut() {
                    edits[i][j] = best.2;
                }
            }
        }

        DpOutput {
            score: dists[len1][len2],
            edits,
        }
    }

    fn backtrack_alignment<'a>(
        s1: &'a [Phoneme],
        s2: &'a [Phoneme],
        edits: &[Vec<Edit>],
    ) -> Vec<AlignmentStep<'a>> {
        let mut i = s1.len();
        let mut j = s2.len();
        let mut alignment = Vec::new();

        while i > 0 || j > 0 {
            match edits[i][j] {
                Edit::Start => break,
                Edit::GapInFirst => {
                    j -= 1;
                    alignment.push(AlignmentStep::Indel {
                        a: None,
                        b: Some(&s2[j]),
                    });
                }
                Edit::GapInSecond => {
                    i -= 1;
                    alignment.push(AlignmentStep::Indel {
                        a: Some(&s1[i]),
                        b: None,
                    });
                }
                Edit::Substitute => {
                    i -= 1;
                    j -= 1;
                    let step = if s1[i] == s2[j] {
                        AlignmentStep::Match {
                            a: &s1[i],
                            b: &s2[j],
                        }
                    } else {
                        AlignmentStep::Substitution {
                            a: &s1[i],
                            b: &s2[j],
                        }
                    };
                    alignment.push(step);
                }
                Edit::Transpose => {
                    // Adjacent swap of s1[i-2],s1[i-1] with s2[j-2],s2[j-1]:
                    // emit the later crossing pair then the earlier one (the
                    // final reverse orders them earlier-first), and step back
                    // two cells in each sequence.
                    let i1 = i - 1;
                    let i2 = j - 1;

                    alignment.push(AlignmentStep::Transposition {
                        a: &s1[i1],
                        b: &s2[i2],
                        direction: TranspositionDirection::Left,
                    });
                    alignment.push(AlignmentStep::Transposition {
                        a: &s1[i1 - 1],
                        b: &s2[i2 - 1],
                        direction: TranspositionDirection::Right,
                    });

                    i -= 2;
                    j -= 2;
                }
            }
        }

        alignment.reverse();
        alignment
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
        let prefix = slice1
            .iter()
            .zip(slice2.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let prefix_context: Option<&Phoneme> = if prefix > 0 {
            Some(&slice1[prefix - 1])
        } else {
            None
        };
        let s1 = &slice1[prefix..];
        let s2 = &slice2[prefix..];

        let dp = self.run_dp(s1, s2, prefix_context, true);
        let edits = dp
            .edits
            .as_ref()
            .expect("traceback edits are recorded when trace is enabled");
        let mut alignment = Self::backtrack_alignment(s1, s2, edits);

        for i in (0..prefix).rev() {
            alignment.insert(
                0,
                AlignmentStep::Match {
                    a: &slice1[i],
                    b: &slice2[i],
                },
            );
        }

        RhymeDistanceOutput {
            score: dp.score,
            alignment,
        }
    }

    pub fn distance(&self, slice1: &[Phoneme], slice2: &[Phoneme]) -> u32 {
        // Strip common prefix. Only the prefix: see `align` for why prefix
        // stripping preserves optimality while suffix stripping would not.
        let prefix = slice1
            .iter()
            .zip(slice2.iter())
            .take_while(|(a, b)| a == b)
            .count();
        // The element just before the stripped region — needed for repeated-consonant gap detection.
        let prefix_context: Option<&Phoneme> = if prefix > 0 {
            Some(&slice1[prefix - 1])
        } else {
            None
        };
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

        self.run_dp(s1, s2, prefix_context, false).score
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Consonant, Vowel,
        consonant_distance::{AnalyticConsonantDistanceConfig, ConsonantDistanceScorer},
        vowel_distance::{VowelDistanceScorer, VowelHexGraphConfig},
    };
    use std::sync::OnceLock;

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

    fn vowel_config() -> VowelHexGraphConfig {
        VowelHexGraphConfig::default()
    }

    fn consonant_config() -> AnalyticConsonantDistanceConfig {
        AnalyticConsonantDistanceConfig::default()
    }

    fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
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
                    prev_a = Some(a);
                    prev_b = Some(b);
                    s.substitution_score(a, b)
                }
                AlignmentStep::Indel {
                    a: Some(a),
                    b: None,
                } => {
                    let c = s.gap_penalty(a, prev_a);
                    prev_a = Some(a);
                    c
                }
                AlignmentStep::Indel {
                    a: None,
                    b: Some(b),
                } => {
                    let c = s.gap_penalty(b, prev_b);
                    prev_b = Some(b);
                    c
                }
                AlignmentStep::Indel { .. } => unreachable!("an indel has exactly one side"),
                AlignmentStep::Transposition { a, b, direction } => {
                    // The step's `a`/`b` are the column phonemes (which differ),
                    // but the DP matches them diagonally across the swap at cost
                    // 0 — a transposition only fires when the swapped phonemes
                    // are equal. So the endpoints add nothing; the fixed
                    // transposition_cost is counted once, on the closing (Left)
                    // step of the pair.
                    prev_a = Some(a);
                    prev_b = Some(b);
                    if direction == TranspositionDirection::Left {
                        s.config().transposition_cost
                    } else {
                        0
                    }
                }
            };
            reference.push(cost);
        }

        // 1. The independent derivation must sum to the DP's score — this is the
        //    forward/traceback consistency guard.
        assert_eq!(
            reference.iter().sum::<u32>(),
            output.score(),
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
    /// (delete / insert / substitute / adjacent transposition), sharing the
    /// cost primitives but none of the machinery under test: no matrix, no
    /// boundary initialization, no prefix/suffix stripping, no slice swapping.
    /// This is an *implementation* oracle — it validates that run_dp and its
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
        // Adjacent transposition: s1[i],s1[i+1] crossed with s2[j],s2[j+1].
        if i + 1 < s1.len() && j + 1 < s2.len() && s1[i] == s2[j + 1] && s1[i + 1] == s2[j] {
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
                s.align(&s1, &s2).score(),
                expected,
                "align() score diverged from the exhaustive oracle for {s1:?} vs {s2:?}"
            );
        }
    }

    #[test]
    fn interior_gap_charges_the_gapped_phoneme() {
        // Regression: run_dp once swapped the insert/delete gap costs, charging
        // an interior gap by the *other* sequence's current phoneme. That let a
        // vowel be indel'd for the price of an adjacent consonant, dodging the
        // vowel_indel_penalty (e.g. "blueprint" vs "new bag" scored far too low).
        //
        // Matching consonants on both sides force the vowel to be an interior
        // gap. Both orientations are checked: inserting the vowel exercises the
        // GapInFirst branch, deleting it exercises GapInSecond.
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
        // consonant gap = 5 each, vowel gap = 20
        // M(5) + AO1(20) + S(5) + T(5) = 35
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
        // AO1 vs OW1: adjacent on hex-graph (dist=1), same stress
        // cost = (1 * VOWEL_DISTANCE_COEFFICIENT + 0) * VOWEL_COEFFICIENT
        let s1 = parse("M AO1 S T");
        let s2 = parse("M OW1 S T");
        assert_eq!(
            scorer().distance(&s1, &s2),
            vowel_config().vowel_distance_coefficient * config().vowel_coefficient
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
        let config = RhymeDistanceConfig {
            vowel_to_consonant_mismatch: 5,
            vowel_coefficient: 4,
            consonant_coefficient: 4,
            vowel_indel_penalty: 40,
            consonant_indel_penalty: 6,
            consonant_repeated_penalty: 2,
            transposition_cost: 3,
        };
        let scorer = RhymeDistanceScorer::with_config(config);

        assert_eq!(
            scorer.distance(&parse("AO1"), &parse("OW1")),
            vowel_config().vowel_distance_coefficient * config.vowel_coefficient
        );
        assert_eq!(scorer.distance(&parse("AO1"), &parse("B")), 5);
        assert_eq!(
            scorer.distance(&parse("T"), &parse("D")),
            consonant_config().voiced_penalty * config.consonant_coefficient
        );
        assert_eq!(scorer.distance(&parse("B"), &[]), 6);
        assert_eq!(scorer.distance(&parse("B B"), &parse("B")), 2);
        assert_eq!(scorer.distance(&parse("M OW1 S T"), &parse("M OW1 T S")), 3);
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
            consonant_config().voiced_penalty * config.consonant_coefficient
        );
    }

    #[test]
    fn consonant_substitution_voiced_pair() {
        // T vs D: same place (4), voiced penalty only
        let s1 = parse("T AE1 N");
        let s2 = parse("D AE1 N");
        // T vs D: same manner (Plosive), place diff 0, voiced penalty 1
        assert_eq!(
            scorer().distance(&s1, &s2),
            consonant_config().voiced_penalty * config().consonant_coefficient
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
}
